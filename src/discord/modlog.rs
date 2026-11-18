//! #mod-log: every change staff or noxaDBRS make, one line each, no pings. A change runs only once the log
//! is set, so none goes unlogged:
//!
//! ```text
//!   command ─► channel() ─None─► refused (NO_MOD_LOG)
//!                │ Some
//!                ▼
//!           service change ─► write(line)
//! ```

use anyhow::Result;
use tracing::{error, info, warn};
use twilight_model::channel::message::AllowedMentions;
use twilight_model::id::Id;
use twilight_model::id::marker::ChannelMarker;

use super::app::App;

pub(crate) const NO_MOD_LOG: &str =
    "Set the mod log first: `/z_moderator_dbrs action:mod_log channel:#mod-log`. Changes wait for it, so none goes unlogged.";

pub(crate) fn channel(app: &App) -> Result<Option<Id<ChannelMarker>>> {
    Ok(app.services.settings().mod_log()?.and_then(Id::new_checked))
}

/// The channel a change must log to; a refusal is logged when none is set.
pub(crate) fn required(app: &App) -> Result<Option<Id<ChannelMarker>>> {
    let found = channel(app)?;
    if found.is_none() {
        warn!("change refused: the mod log is not set");
    }
    Ok(found)
}

/// Posts the line; false when Discord refused it (no access, channel gone). The journal keeps every
/// line either way, so a change is never unrecorded.
pub(crate) async fn post(app: &App, channel: Id<ChannelMarker>, line: &str) -> bool {
    info!(target: "audit", %channel, %line, "change");
    let mentions = AllowedMentions::default();
    let sent = app.http.create_message(channel).content(line).allowed_mentions(Some(&mentions)).await;
    if let Err(source) = &sent {
        error!(target: "audit", ?source, %channel, %line, "the mod log refused the line; it is kept here");
    }
    sent.is_ok()
}

/// Where a change was made: " · #bot-commands".
pub(crate) fn where_of(channel: Option<Id<ChannelMarker>>) -> String {
    channel.map(|id| format!(" · <#{id}>")).unwrap_or_default()
}
