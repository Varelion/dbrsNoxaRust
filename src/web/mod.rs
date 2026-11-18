//! The web pages, behind links sent by DM: the admin pages for staff (/a/), a player's own page (/p/).
//! A second UI beside Discord, over the same services; Discord itself is reached only through
//! `services::guild`. A link opens only its own kind of page.
//!
//! ```text
//!   GET  /p/<token>                                a player: their characters, who they play, Spirit Points
//!   POST /p/<token>/play                           play another of their characters
//!   GET  /p/<token>/players[/<id>]                 everyone's characters, as cards
//!   GET  /a/<token>                                home: counts, mod log
//!   GET  /a/<token>/players[/<id>]                 players, one player: Spirit Points, ledger, characters
//!   POST /a/<token>/players/<id>/sp|characters     change Spirit Points, add a character
//!   GET  /a/<token>/characters[/<id>]              every character, one sheet: belongings, feats
//!   POST /a/<token>/characters/<id>[/picture|feats|mcguffins]  save, upload a picture, log a feat, give
//!   GET  /a/<token>/npcs     POST /a/<token>/npcs  the cast, add an NPC
//!   POST /a/<token>/feats/<id>/archive|restore
//!   GET  /a/<token>/mcguffins[/<id>]               POST …/give|take|retire, create, edit
//!   GET  /a/<token>/balls[/<set>]                  POST …/need|areas|place|scatter
//!   GET  /healthz                                  "ok"
//! ```
//!
//! A form posts, the change is made, and the browser is sent back (303) to the page it came from,
//! with what happened shown once. Every change waits for the mod log, like in Discord, and its line
//! goes there through the guild port.

mod balls;
mod html;
mod mcguffins;
mod me;
mod players;
pub(crate) mod relay;
mod sheets;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use tracing::{error, info};

use crate::domain::pictures::UPLOAD_MAX;
use crate::services::Services;
use crate::services::guild::Guild;
use crate::services::links::{Access, Notice};

pub(crate) const NO_MOD_LOG: &str =
    "Set the mod log first, in Discord: /z_moderator_dbrs action:mod_log. Changes wait for it, so none goes unlogged.";
// Locks the pages down: no scripts at all, pictures only over https, forms only back here, never framed.
const CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; img-src https:; form-action 'self'; frame-ancestors 'none'; base-uri 'none'";
// An upload is the picture plus the form's own few lines.
const UPLOAD_BODY_MAX: usize = UPLOAD_MAX + 64 * 1024;
/// Ends every mod log line made here, so staff know where a change came from.
const VIA: &str = " · admin pages";

#[derive(Clone)]
struct Web {
    services: Arc<Services>,
    guild: Guild,
}

/// A staff member on a live link.
struct Session {
    token: String,
    staff: u64,
}

impl Session {
    /// A path under this link: at("/players") is "/a/<token>/players".
    fn at(&self, path: &str) -> String {
        format!("/a/{}{path}", self.token)
    }
}

impl Web {
    /// A staff member, if the token is a live admin link.
    fn staff(&self, token: &str) -> Option<Session> {
        let staff = self.services.links().open(token, Access::Staff)?;
        Some(Session { token: token.to_owned(), staff })
    }

    /// The notice a form left for this page, if any.
    fn told(&self, session: &Session) -> Option<Notice> {
        self.services.links().told(&session.token)
    }

    /// Back to `path` after a form, with what happened shown there once.
    fn back(&self, session: &Session, path: &str, notice: Notice) -> Response {
        self.back_to(&session.token, &session.at(path), notice)
    }

    /// To `url` (a whole path), with a notice for the link's next page.
    fn back_to(&self, token: &str, url: &str, notice: Notice) -> Response {
        self.services.links().tell(token, notice);
        Redirect::to(url).into_response()
    }

    /// A change is refused until the mod log is set.
    fn logged(&self) -> Result<bool> {
        Ok(self.services.settings().mod_log()?.is_some())
    }

    fn audit(&self, line: &str) {
        info!(target: "audit", %line, "change on the admin pages");
        self.guild.audit(format!("{line}{VIA}"));
    }
}

/// A page that failed: logged with its causes; the visitor sees a plain page.
struct Failure(anyhow::Error);

impl<E: Into<anyhow::Error>> From<E> for Failure {
    fn from(source: E) -> Self {
        Self(source.into())
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        error!(error = ?self.0, "admin page failed");
        (StatusCode::INTERNAL_SERVER_ERROR, Html(html::failed())).into_response()
    }
}

type Page = Result<Response, Failure>;

fn expired() -> Response {
    (StatusCode::NOT_FOUND, Html(html::expired())).into_response()
}

/// Whose a character is, for a mod log line: their player, or the cast.
fn owner(player_id: u64) -> String {
    if player_id == crate::domain::npcs::CAST {
        return "NPC".to_owned();
    }
    format!("<@{player_id}>")
}

fn done(text: impl Into<String>) -> Notice {
    Notice::Done(text.into())
}

fn refused(text: impl Into<String>) -> Notice {
    Notice::Refused(text.into())
}

pub(crate) fn routes(services: Arc<Services>, guild: Guild) -> Router {
    let upload = post(sheets::upload).layer(DefaultBodyLimit::max(UPLOAD_BODY_MAX));
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/a/{token}", get(home))
        .route("/a/{token}/players", get(players::list))
        .route("/a/{token}/players/find", get(players::find))
        .route("/a/{token}/players/{player}", get(players::show))
        .route("/a/{token}/players/{player}/sp", post(players::spirit))
        .route("/a/{token}/players/{player}/characters", post(players::add_character))
        .route("/a/{token}/characters", get(sheets::list))
        .route("/a/{token}/characters/{id}", get(sheets::show).post(sheets::save))
        .route("/a/{token}/characters/{id}/picture", upload)
        .route("/a/{token}/characters/{id}/feats", post(sheets::add_feat))
        .route("/a/{token}/characters/{id}/mcguffins", post(sheets::give_mcguffin))
        .route("/a/{token}/npcs", get(sheets::npcs).post(sheets::add_npc))
        .route("/a/{token}/feats/{id}/archive", post(sheets::archive))
        .route("/a/{token}/feats/{id}/restore", post(sheets::restore))
        .route("/a/{token}/mcguffins", get(mcguffins::list).post(mcguffins::create))
        .route("/a/{token}/mcguffins/{id}", get(mcguffins::show).post(mcguffins::edit))
        .route("/a/{token}/mcguffins/{id}/give", post(mcguffins::give))
        .route("/a/{token}/mcguffins/{id}/take", post(mcguffins::take))
        .route("/a/{token}/mcguffins/{id}/retire", post(mcguffins::retire))
        .route("/a/{token}/balls", get(balls::list))
        .route("/a/{token}/balls/{set}", get(balls::show))
        .route("/a/{token}/balls/{set}/need", post(balls::need))
        .route("/a/{token}/balls/{set}/areas", post(balls::areas))
        .route("/a/{token}/balls/{set}/place", post(balls::place))
        .route("/a/{token}/balls/{set}/scatter", post(balls::scatter))
        .route("/p/{token}", get(me::home))
        .route("/p/{token}/play", post(me::play))
        .route("/p/{token}/players", get(me::players))
        .route("/p/{token}/players/{player}", get(me::player))
        .layer(axum::middleware::map_response(secured))
        .with_state(Web { services, guild })
}

pub(crate) async fn serve(bind: SocketAddr, routes: Router) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(bind).await.with_context(|| format!("binding the admin pages to {bind}"))?;
    info!(%bind, "admin pages listening");
    axum::serve(listener, routes).await.context("serving the admin pages")
}

// The token is in the URL: never cached, never sent on as a referrer.
async fn secured(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    response
}

async fn home(State(web): State<Web>, Path(token): Path<String>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let services = &web.services;
    let players = services.players().all()?;
    let sheets = services.sheets().all()?;
    let cast = sheets.iter().filter(|c| c.player_id == crate::domain::npcs::CAST).count();
    let mcguffins = services.mcguffins().all()?;
    let in_play = mcguffins.iter().filter(|m| !m.retired).count();
    let sets = crate::domain::dragon_balls::BallSet::ALL.iter().map(|set| services.balls().view(*set)).collect::<Result<Vec<_>>>()?;

    let mod_log = match services.settings().mod_log()? {
        Some(id) => {
            let channels = web.guild.channels().await;
            let name = channels.iter().find(|c| c.id == id).map_or_else(|| id.to_string(), |c| c.name.clone());
            html::ModLog::Set(name)
        }
        None => html::ModLog::Unset,
    };
    let counts = html::Counts { players: players.len(), characters: sheets.len() - cast, cast, mcguffins: in_play, sets: &sets };
    let told = web.told(&session);
    Ok(Html(html::home(&session.token, told.as_ref(), &counts, &mod_log)).into_response())
}

#[cfg(test)]
mod tests;
