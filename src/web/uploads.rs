//! Image uploads: a character's picture or one of its expressions, by staff on a sheet or by a
//! player on their own page. The image never reaches the bot (it runs on a Raspberry Pi): the page
//! shrinks it, the bot signs a ticket, and the relay Worker posts it to #pictures through a webhook,
//! then hands the bot the link. The bot keeps the link, signs it again whenever it is used, and logs
//! every upload with a mod log line linking the message, so staff see each image.
//!
//! ```text
//!   page ─ POST …/characters/<id>/upload-ticket {target, name?, color?} ─► bot: checks, signs (5 min)
//!   page ─ ≤1024 px WebP ─ POST /upload, X-Upload-Ticket ─► Worker ─► #pictures "🖼️ Captain Nova · …"
//!   Worker ─ trusted frame: POST /api/uploaded {ticket, url, messageId, channelId} ─► bot:
//!        ticket checked again, spent ─► the picture or expression saved ─► 🖼️ mod log line
//!   page ◄─ { ok, message } ◄─ Worker
//! ```
//!
//! The ticket fixes the character, the target and the caption: the Worker trusts nothing the page says.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::Extension;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use tracing::{error, warn};

use super::html::{esc, plain};
use super::sheets::expression_refusal;
use super::{NO_MOD_LOG, Page, Trusted, Web, owner};
use crate::domain::proxy::clean_expression;
use crate::domain::tickets::{Broken, By, Grant, Kind};
use crate::services::guild::Uploaded;
use crate::services::links::Access;
use crate::services::sheets::{ExpressionSaved, expression_refused};
use crate::services::tickets::Refused;
use crate::store::characters::CharacterRow;

const PICTURE_FIELD: &str = "picture";
const NAME_FIELD: &str = "name";
const COLOR_FIELD: &str = "color";
const ACCEPT: &str = "image/png,image/jpeg,image/gif,image/webp";
const JSON: &str = "application/json";
const SCRIPT_TYPE: &str = "text/javascript; charset=utf-8";
const SECURE_LINK: &str = "https://";
/// The page script: shrink, ticket, send. Served at UPLOAD_SCRIPT_PATH.
const SCRIPT: &str = include_str!("upload.js");
pub(super) const UPLOAD_SCRIPT_PATH: &str = "/upload.js";
/// One per page that has an upload form; the script binds every form marked data-upload.
pub(super) const UPLOAD_SCRIPT: &str = "<script src=\"/upload.js\" defer></script>";
const GONE: &str = "That character is gone.";
const NOT_YOURS: &str = "Pick one of your own characters.";
const EXPIRED: &str = "This link has expired. Ask for a new one.";
const NO_RELAY: &str = "Uploads go through the relay, and this bot has none (RELAY_URL).";
const BAD_ASK: &str = "That upload request was not understood.";
const BAD_TICKET: &str = "That upload ticket is not valid. Try again.";
const OLD_TICKET: &str = "That upload took over 5 minutes. Try again.";
const SPENT_TICKET: &str = "That upload was already saved.";
const SAVE_FAILED: &str = "Saving the picture failed. It is in the bot's log.";
/// Ends a player's mod log line, so staff know where it came from.
const VIA_PAGE: &str = " · /mypage";

/// The form for a character's picture; `ticket` is where it asks for its upload ticket.
pub(super) fn picture_form(ticket: &str) -> String {
    format!(
        "<form data-upload=\"picture\" data-ticket=\"{}\" class=\"row\">\
         <label>New picture: PNG, JPEG, GIF or WebP<input type=\"file\" name=\"{PICTURE_FIELD}\" accept=\"{ACCEPT}\" required></label>\
         <button type=\"submit\">Upload</button><output class=\"small\" role=\"status\"></output></form>\
         <p class=\"dim small\">Shrunk to 1024 px in your browser, then kept as a message in the pictures channel, so Discord hosts it.</p>",
        esc(ticket)
    )
}

/// The form for an expression's picture, uploaded; `ticket` is where it asks for its upload ticket.
pub(super) fn expression_form(ticket: &str) -> String {
    format!(
        "<form data-upload=\"expression\" data-ticket=\"{}\" class=\"grid\">\
         <label>Name<input name=\"{NAME_FIELD}\" maxlength=\"20\" placeholder=\"happy\" required></label>\
         <label>Image<input type=\"file\" name=\"{PICTURE_FIELD}\" accept=\"{ACCEPT}\" required></label>\
         <label>Colour bar, optional<input name=\"{COLOR_FIELD}\" maxlength=\"7\" placeholder=\"#C0392B\"></label>\
         <div><button type=\"submit\">Upload expression</button> <output class=\"small\" role=\"status\"></output></div></form>",
        esc(ticket)
    )
}

/// What the page asks a ticket for: `{"target":"expression","name":"happy","color":"#C0392B"}`.
#[derive(Deserialize)]
struct Asked {
    target: Kind,
    #[serde(default)]
    name: String,
    #[serde(default)]
    color: String,
}

/// What the Worker hands back once Discord has the file.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Posted {
    ticket: String,
    url: String,
    message_id: String,
    channel_id: String,
}

/// Every answer here is JSON for the page script: `{ ok, ticket? , message?, error? }`.
#[derive(Serialize, Default)]
struct Answer {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    ticket: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn json(status: StatusCode, answer: &Answer) -> Response {
    let body = serde_json::to_string(answer).unwrap_or_default();
    (status, [(header::CONTENT_TYPE, JSON)], body).into_response()
}

fn no(status: StatusCode, why: impl Into<String>) -> Response {
    json(status, &Answer { error: Some(why.into()), ..Answer::default() })
}

fn yes(message: impl Into<String>) -> Response {
    json(StatusCode::OK, &Answer { ok: true, message: Some(message.into()), ..Answer::default() })
}

fn now_s() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

pub(super) async fn script() -> Response {
    ([(header::CONTENT_TYPE, SCRIPT_TYPE)], SCRIPT).into_response()
}

// The caption the Worker posts with the image: whose it is, what it becomes, who sent it.
fn caption(row: &CharacterRow, sender: u64, expression: Option<&str>) -> String {
    let label = expression.map_or_else(|| "picture".to_owned(), |name| format!("`{name}`"));
    format!("🖼️ **{}** · {label} ({}) · uploaded by <@{sender}>", plain(&row.name), owner(row.player_id))
}

// What the page asks for, checked before a ticket is signed. Err: why not.
fn grant_of(web: &Web, row: &CharacterRow, sender: u64, by: By, body: &[u8]) -> Result<Grant, String> {
    let asked: Asked = serde_json::from_slice(body).map_err(|_| BAD_ASK.to_owned())?;
    let expression = match asked.target {
        Kind::Picture => None,
        Kind::Expression => {
            if let Some(refusal) = expression_refused(&asked.name, &asked.color) {
                return Err(expression_refusal(&refusal).to_owned());
            }
            Some(clean_expression(&asked.name).unwrap_or(asked.name))
        }
    };
    let caption = caption(row, sender, expression.as_deref());
    let color = if asked.target == Kind::Expression { asked.color } else { String::new() };
    Ok(Grant { guild: web.guild.server(), uploader: sender, owner: row.player_id, character: row.id, kind: asked.target, expression, caption, by, color })
}

// A signed ticket for the page, or why not.
fn ticket_for(web: &Web, row: &CharacterRow, sender: u64, by: By, body: &[u8]) -> Response {
    let Some(tickets) = web.tickets.as_deref() else {
        return no(StatusCode::SERVICE_UNAVAILABLE, NO_RELAY);
    };
    let grant = match grant_of(web, row, sender, by, body) {
        Ok(grant) => grant,
        Err(why) => return no(StatusCode::UNPROCESSABLE_ENTITY, why),
    };
    match tickets.issue(grant, now_s()) {
        Ok(ticket) => json(StatusCode::OK, &Answer { ok: true, ticket: Some(ticket), ..Answer::default() }),
        Err(source) => {
            error!(error = ?source, "signing an upload ticket failed");
            no(StatusCode::INTERNAL_SERVER_ERROR, SAVE_FAILED)
        }
    }
}

// Staff: any character's picture or expression, once the mod log is set.
pub(super) async fn staff_ticket(State(web): State<Web>, Path((token, id)): Path<(String, i64)>, body: Bytes) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(no(StatusCode::NOT_FOUND, EXPIRED));
    };
    if !web.logged()? {
        return Ok(no(StatusCode::CONFLICT, NO_MOD_LOG));
    }
    let Some(row) = web.services.sheets().get(id)? else {
        return Ok(no(StatusCode::NOT_FOUND, GONE));
    };
    Ok(ticket_for(&web, &row, session.staff, By::Staff, &body))
}

// A player: their own character's picture or expression.
pub(super) async fn my_ticket(State(web): State<Web>, Path((token, id)): Path<(String, i64)>, body: Bytes) -> Page {
    let Some(player) = web.services.links().open(&token, Access::Player) else {
        return Ok(no(StatusCode::NOT_FOUND, EXPIRED));
    };
    let Some(row) = web.services.sheets().owned(player, id)? else {
        return Ok(no(StatusCode::FORBIDDEN, NOT_YOURS));
    };
    Ok(ticket_for(&web, &row, player, By::Player, &body))
}

// The character the grant names, still there and, for a player, still theirs. Err: why not.
fn row_for(web: &Web, grant: &Grant) -> anyhow::Result<Result<CharacterRow, &'static str>> {
    let sheets = web.services.sheets();
    if grant.by == By::Player {
        return Ok(sheets.owned(grant.uploader, grant.character)?.ok_or(NOT_YOURS));
    }
    if !web.logged()? {
        return Ok(Err(NO_MOD_LOG));
    }
    Ok(sheets.get(grant.character)?.ok_or(GONE))
}

// The upload saved where it belongs: the sheet's picture, or the expression. Err: why not.
fn save(web: &Web, grant: &Grant, uploaded: &Uploaded) -> anyhow::Result<Result<CharacterRow, &'static str>> {
    let sheets = web.services.sheets();
    let Some(name) = grant.expression.as_deref().filter(|_| grant.kind == Kind::Expression) else {
        return Ok(sheets.set_picture(grant.character, &uploaded.url, uploaded.message_id)?.ok_or(GONE));
    };
    match sheets.save_expression(grant.character, name, &uploaded.url, &grant.color)? {
        ExpressionSaved::Done(_) => Ok(sheets.get(grant.character)?.ok_or(GONE)),
        other => Ok(Err(expression_refusal(&other))),
    }
}

// The mod log line for a saved upload, linking its message in #pictures.
fn audit(web: &Web, grant: &Grant, row: &CharacterRow, jump: &str) {
    let what = match grant.expression.as_deref() {
        Some(name) => format!("expression **{}**", plain(name)),
        None => "a picture".to_owned(),
    };
    let (name, sender) = (plain(&row.name), grant.uploader);
    match grant.by {
        By::Staff => web.audit(&format!("🖼️ <@{sender}> uploaded {what} for **{name}** ({}): {jump}", owner(row.player_id))),
        By::Player => web.guild.audit(format!("🖼️ Player image, {what} for **{name}**, submitted by <@{sender}>: {jump}{VIA_PAGE}")),
    }
}

// The Worker's frame read: its ticket's grant, and where Discord put the file. Err: the refusal.
fn posted(web: &Web, body: &[u8]) -> Result<(Grant, Uploaded), (StatusCode, &'static str)> {
    let Ok(posted) = serde_json::from_slice::<Posted>(body) else {
        return Err((StatusCode::BAD_REQUEST, BAD_ASK));
    };
    let ids = (posted.message_id.parse::<u64>(), posted.channel_id.parse::<u64>());
    let (Ok(message_id), Ok(channel_id)) = ids else {
        return Err((StatusCode::BAD_REQUEST, BAD_ASK));
    };
    if !posted.url.starts_with(SECURE_LINK) {
        return Err((StatusCode::BAD_REQUEST, BAD_ASK));
    }
    let Some(tickets) = web.tickets.as_deref() else {
        return Err((StatusCode::SERVICE_UNAVAILABLE, NO_RELAY));
    };
    let grant = match tickets.redeem(&posted.ticket, now_s()) {
        Ok(grant) => grant,
        Err(Refused::Broken(Broken::Expired)) => return Err((StatusCode::FORBIDDEN, OLD_TICKET)),
        Err(Refused::Broken(_)) => return Err((StatusCode::FORBIDDEN, BAD_TICKET)),
        Err(Refused::Spent) => return Err((StatusCode::CONFLICT, SPENT_TICKET)),
    };
    // A ticket signed for another server (a second bot sharing the secret) is not this one's.
    if grant.guild != web.guild.server() {
        return Err((StatusCode::FORBIDDEN, BAD_TICKET));
    }
    Ok((grant, Uploaded { channel_id, message_id, url: posted.url }))
}

/// POST /api/uploaded: only the relay Worker's own frame (trusted) reaches it; for anyone else it is not here.
pub(super) async fn uploaded(State(web): State<Web>, trusted: Option<Extension<Trusted>>, body: Bytes) -> Page {
    if trusted.is_none() {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    let (grant, uploaded) = match posted(&web, &body) {
        Ok(read) => read,
        Err((status, why)) => return Ok(no(status, why)),
    };
    let saved = row_for(&web, &grant).and_then(|row| match row {
        Ok(_) => save(&web, &grant, &uploaded),
        Err(why) => Ok(Err(why)),
    });
    let row = match saved {
        Ok(Ok(row)) => row,
        Ok(Err(why)) => {
            warn!(character = grant.character, %why, "an upload reached #pictures but was not saved");
            return Ok(no(StatusCode::UNPROCESSABLE_ENTITY, why));
        }
        Err(source) => {
            error!(error = ?source, "saving an upload failed");
            return Ok(no(StatusCode::INTERNAL_SERVER_ERROR, SAVE_FAILED));
        }
    };
    audit(&web, &grant, &row, &uploaded.jump(web.guild.server()));
    match grant.expression {
        Some(name) => Ok(yes(format!("The {name} expression is up."))),
        None => Ok(yes("The new picture is up.")),
    }
}
