//! Image uploads: a character's picture or one of its expressions, by staff on a sheet or by a
//! player on their own page. Discord hosts the file in the pictures channel; its link is kept and
//! signed again whenever it is used. Every upload's mod log line links the message, so staff see it.
//!
//! ```text
//!   [file] ─► PNG | JPEG | GIF | WebP, up to 8 MiB ─► back at once: "Uploading…"
//!          └─ in the background ─► #pictures "🖼️ Captain Nova · happy · @player"
//!                               ─► the picture or expression saved ─► 🖼️ mod log line
//!                               ─► how it went, on the next page the link opens
//! ```

use std::collections::HashMap;

use axum::extract::multipart::MultipartError;
use axum::extract::{Multipart, Path, State};
use tracing::error;

use super::html::{esc, plain};
use super::sheets::expression_refusal;
use super::{NO_MOD_LOG, Page, Web, done, expired, owner, refused};
use crate::domain::pictures::{ImageKind, UPLOAD_MAX, kind_of};
use crate::domain::proxy::clean_expression;
use crate::services::guild::{Upload, UploadRefused};
use crate::services::links::{Access, Notice};
use crate::services::sheets::{ExpressionSaved, expression_refused};
use crate::store::characters::CharacterRow;

const PICTURE_FIELD: &str = "picture";
const NAME_FIELD: &str = "name";
const COLOR_FIELD: &str = "color";
const UPLOAD_MIB: usize = UPLOAD_MAX / (1024 * 1024);
const ACCEPT: &str = "image/png,image/jpeg,image/gif,image/webp";
const GONE: &str = "That character is gone.";
const NOT_YOURS: &str = "Pick one of your own characters.";
const UPLOADING: &str = "Uploading… reload in a few seconds to see it.";
/// Ends a player's mod log line, so staff know where it came from.
const VIA_PAGE: &str = " · /mypage";

/// The form for a character's picture; `action` is where it posts.
pub(super) fn picture_form(action: &str) -> String {
    format!(
        "<form method=\"post\" action=\"{}\" enctype=\"multipart/form-data\" class=\"row\">\
         <label>New picture: PNG, JPEG, GIF or WebP, up to {UPLOAD_MIB} MiB<input type=\"file\" name=\"{PICTURE_FIELD}\" accept=\"{ACCEPT}\" required></label>\
         <button type=\"submit\">Upload</button></form>\
         <p class=\"dim small\">Kept as a message in the pictures channel, so Discord hosts it.</p>",
        esc(action)
    )
}

/// The form for an expression's picture, uploaded; `action` is where it posts.
pub(super) fn expression_form(action: &str) -> String {
    format!(
        "<form method=\"post\" action=\"{}\" enctype=\"multipart/form-data\" class=\"grid\">\
         <label>Name<input name=\"{NAME_FIELD}\" maxlength=\"20\" placeholder=\"happy\" required></label>\
         <label>Image, up to {UPLOAD_MIB} MiB<input type=\"file\" name=\"{PICTURE_FIELD}\" accept=\"{ACCEPT}\" required></label>\
         <label>Colour bar, optional<input name=\"{COLOR_FIELD}\" maxlength=\"7\" placeholder=\"#C0392B\"></label>\
         <div><button type=\"submit\">Upload expression</button></div></form>",
        esc(action)
    )
}

/// Who sent an upload: staff on the admin pages, or the character's player on their page.
#[derive(Clone, Copy, PartialEq, Eq)]
enum By {
    Staff,
    Player,
}

/// What a form uploads.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Wanted {
    Picture,
    Expression,
}

/// What an upload becomes.
enum Target {
    Picture,
    Expression { name: String, color: String },
}

/// An upload on its way: whose character, who sent it, and what it becomes.
struct Job {
    id: i64,
    sender: u64,
    by: By,
    target: Target,
    upload: Upload,
}

/// A form as sent: the image, and its text fields.
struct Sent {
    bytes: Vec<u8>,
    kind: ImageKind,
    texts: HashMap<String, String>,
}

impl Sent {
    fn text(&self, field: &str) -> String {
        self.texts.get(field).cloned().unwrap_or_default()
    }
}

// Every field of the form; Err: it did not arrive whole.
async fn fields(mut multipart: Multipart) -> Result<(Vec<u8>, HashMap<String, String>), MultipartError> {
    let (mut bytes, mut texts) = (Vec::new(), HashMap::new());
    while let Some(field) = multipart.next_field().await? {
        let name = field.name().unwrap_or_default().to_owned();
        if name == PICTURE_FIELD {
            bytes = field.bytes().await?.to_vec();
            continue;
        }
        texts.insert(name, field.text().await?);
    }
    Ok((bytes, texts))
}

// The form's image, checked, or why it is refused.
async fn read(multipart: Multipart) -> Result<Sent, String> {
    let (bytes, texts) = match fields(multipart).await {
        Ok(read) => read,
        Err(source) => {
            tracing::warn!(?source, "an upload did not arrive whole");
            return Err(format!("That upload did not arrive whole. Pictures go up to {UPLOAD_MIB} MiB."));
        }
    };
    if bytes.is_empty() {
        return Err("Pick an image file first.".to_owned());
    }
    if bytes.len() > UPLOAD_MAX {
        return Err(format!("That picture is over {UPLOAD_MIB} MiB."));
    }
    let Some(kind) = kind_of(&bytes) else {
        return Err("That is not a PNG, JPEG, GIF or WebP image.".to_owned());
    };
    Ok(Sent { bytes, kind, texts })
}

// A file's name from the character's, and the expression's: "captain-nova-happy".
fn stem_of(name: &str) -> String {
    let stem: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let stem = stem.split('-').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-");
    if stem.is_empty() { PICTURE_FIELD.to_owned() } else { stem }
}

// What the form asks for, checked before anything goes to Discord.
fn target_of(sent: &Sent, wanted: Wanted) -> Result<Target, String> {
    if wanted == Wanted::Picture {
        return Ok(Target::Picture);
    }
    let (name, color) = (sent.text(NAME_FIELD), sent.text(COLOR_FIELD));
    if let Some(refusal) = expression_refused(&name, &color) {
        return Err(expression_refusal(&refusal).to_owned());
    }
    let name = clean_expression(&name).unwrap_or(name);
    Ok(Target::Expression { name, color })
}

// The job for an upload: its file named and captioned for the pictures channel.
fn job_of(row: &CharacterRow, sender: u64, by: By, target: Target, sent: Sent) -> Job {
    let (label, stem) = match &target {
        Target::Picture => ("picture".to_owned(), stem_of(&row.name)),
        Target::Expression { name, .. } => (format!("`{name}`"), stem_of(&format!("{} {name}", row.name))),
    };
    let caption = format!("🖼️ **{}** · {label} ({}) · uploaded by <@{sender}>", plain(&row.name), owner(row.player_id));
    let upload = Upload { bytes: sent.bytes, kind: sent.kind, caption, stem };
    Job { id: row.id, sender, by, target, upload }
}

// A big file takes Discord a while, longer than the relay waits for a page: it goes up in the
// background, and says how it went on the next page this link opens.
fn send(web: &Web, token: &str, job: Job) {
    let (background, token) = (web.clone(), token.to_owned());
    tokio::spawn(async move {
        let notice = finish(&background, job).await;
        background.services.links().tell(&token, notice);
    });
}

// The mod log line for a saved upload.
fn audit(web: &Web, by: By, sender: u64, target: &Target, row: &CharacterRow, jump: &str) {
    let what = match target {
        Target::Picture => "a picture".to_owned(),
        Target::Expression { name, .. } => format!("expression **{}**", plain(name)),
    };
    let name = plain(&row.name);
    match by {
        By::Staff => web.audit(&format!("🖼️ <@{sender}> uploaded {what} for **{name}** ({}): {jump}", owner(row.player_id))),
        By::Player => web.guild.audit(format!("🖼️ Player image, {what} for **{name}**, submitted by <@{sender}>: {jump}{VIA_PAGE}")),
    }
}

// The upload saved where it belongs: the sheet's picture, or the expression. Err: why not.
fn save(web: &Web, id: i64, target: &Target, url: &str, message_id: u64) -> anyhow::Result<Result<CharacterRow, &'static str>> {
    let sheets = web.services.sheets();
    let Target::Expression { name, color } = target else {
        return Ok(sheets.set_picture(id, url, message_id)?.ok_or(GONE));
    };
    match sheets.save_expression(id, name, url, color)? {
        ExpressionSaved::Done(_) => Ok(sheets.get(id)?.ok_or(GONE)),
        other => Ok(Err(expression_refusal(&other))),
    }
}

async fn finish(web: &Web, job: Job) -> Notice {
    let Job { id, sender, by, target, upload } = job;
    let uploaded = match web.guild.upload(upload).await {
        Ok(uploaded) => uploaded,
        Err(UploadRefused::NoChannel) => return refused("Uploads are off: PICTURES_CHANNEL is not set in the bot's .env."),
        Err(UploadRefused::Refused) => return refused("Discord refused the picture. Can the bot post and attach files in the pictures channel?"),
        Err(UploadRefused::Gone) => return refused("The bot is not connected to Discord right now. Try again shortly."),
    };
    let row = match save(web, id, &target, &uploaded.url, uploaded.message_id) {
        Ok(Ok(row)) => row,
        Ok(Err(why)) => return refused(why),
        Err(source) => {
            error!(error = ?source, "saving an upload failed");
            return refused("Saving the picture failed. It is in the bot's log.");
        }
    };
    audit(web, by, sender, &target, &row, &uploaded.jump(web.guild.server()));
    match target {
        Target::Picture => done("The new picture is up."),
        Target::Expression { name, .. } => done(format!("The {name} expression is up.")),
    }
}

// Staff: any character's picture or expression, once the mod log is set.
async fn by_staff(web: &Web, token: &str, id: i64, multipart: Multipart, wanted: Wanted) -> Page {
    let Some(session) = web.staff(token) else {
        return Ok(expired());
    };
    let path = format!("/characters/{id}");
    if !web.logged()? {
        return Ok(web.back(&session, &path, refused(NO_MOD_LOG)));
    }
    let Some(row) = web.services.sheets().get(id)? else {
        return Ok(web.back(&session, "/characters", refused(GONE)));
    };
    let sent = match read(multipart).await {
        Ok(sent) => sent,
        Err(why) => return Ok(web.back(&session, &path, refused(why))),
    };
    let target = match target_of(&sent, wanted) {
        Ok(target) => target,
        Err(why) => return Ok(web.back(&session, &path, refused(why))),
    };
    send(web, token, job_of(&row, session.staff, By::Staff, target, sent));
    Ok(web.back(&session, &path, done(UPLOADING)))
}

// A player: their own character's picture or expression.
async fn by_player(web: &Web, token: &str, id: i64, multipart: Multipart, wanted: Wanted) -> Page {
    let Some(player) = web.services.links().open(token, Access::Player) else {
        return Ok(expired());
    };
    let home = format!("/p/{token}");
    let Some(row) = web.services.sheets().owned(player, id)? else {
        return Ok(web.back_to(token, &home, refused(NOT_YOURS)));
    };
    let sent = match read(multipart).await {
        Ok(sent) => sent,
        Err(why) => return Ok(web.back_to(token, &home, refused(why))),
    };
    let target = match target_of(&sent, wanted) {
        Ok(target) => target,
        Err(why) => return Ok(web.back_to(token, &home, refused(why))),
    };
    send(web, token, job_of(&row, player, By::Player, target, sent));
    Ok(web.back_to(token, &home, done(UPLOADING)))
}

pub(super) async fn staff_picture(State(web): State<Web>, Path((token, id)): Path<(String, i64)>, multipart: Multipart) -> Page {
    by_staff(&web, &token, id, multipart, Wanted::Picture).await
}

pub(super) async fn staff_expression(State(web): State<Web>, Path((token, id)): Path<(String, i64)>, multipart: Multipart) -> Page {
    by_staff(&web, &token, id, multipart, Wanted::Expression).await
}

pub(super) async fn my_picture(State(web): State<Web>, Path((token, id)): Path<(String, i64)>, multipart: Multipart) -> Page {
    by_player(&web, &token, id, multipart, Wanted::Picture).await
}

pub(super) async fn my_expression(State(web): State<Web>, Path((token, id)): Path<(String, i64)>, multipart: Multipart) -> Page {
    by_player(&web, &token, id, multipart, Wanted::Expression).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_come_from_the_character_name() {
        assert_eq!(stem_of("Captain Nova"), "captain-nova");
        assert_eq!(stem_of("  Gogeta (SSJ4)!! "), "gogeta-ssj4");
        assert_eq!(stem_of("孫悟空"), "picture");
        assert_eq!(stem_of("Captain Nova happy"), "captain-nova-happy");
    }
}
