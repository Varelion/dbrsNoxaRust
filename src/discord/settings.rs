//! /z_moderator_dbrs: noxaDBRS's settings.
//!
//!   action:mod_log channel:#mod-log   where every change is posted; the old channel is told it moved

use anyhow::Result;

use super::ctx::{Ctx, Options};
use super::modlog;

const CANNOT_POST: &str = "I cannot post there: give me View Channel and Send Messages, or pick another channel.";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsSub {
    ModLog,
}

impl SettingsSub {
    pub(crate) const ALL: [Self; 1] = [Self::ModLog];

    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::ModLog => "mod_log",
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::ModLog => "Mod log: where every change is posted",
        }
    }
}

pub(crate) async fn admin(ctx: &Ctx<'_>, opts: &Options<'_>) -> Result<()> {
    if !ctx.is_staff() {
        return ctx.deny().await;
    }
    ctx.defer().await?;
    if opts.sub() != Some(SettingsSub::ModLog.key()) {
        return ctx.edit("Pick an action.").await;
    }

    let (Some(staff), Some(channel)) = (ctx.user_id(), opts.channel("channel")) else {
        return ctx.edit("Pick a channel.").await;
    };
    let before = modlog::channel(ctx.app)?;
    let was = before.map(|old| format!(" (was <#{old}>)")).unwrap_or_default();
    let line = format!("⚙️ <@{staff}> set the mod log to <#{channel}>{was}.");

    // Posting the line is the test: a channel noxaDBRS cannot write to is refused before it is saved.
    if !modlog::post(ctx.app, channel, &line).await {
        return ctx.edit(CANNOT_POST).await;
    }
    ctx.app.services.settings().set_mod_log(channel.get())?;
    if let Some(old) = before.filter(|old| *old != channel) {
        modlog::post(ctx.app, old, &line).await;
    }
    ctx.edit(&format!("The mod log is <#{channel}>.")).await
}
