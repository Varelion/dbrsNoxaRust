//! The web links, sent by DM: /z_moderator_admin for the admin pages, /mypage for a player's own page.
//!
//! ```text
//!   /z_moderator_admin ─ staff? ─► issue(staff, Staff)   ─► DM "<https://dbrs.neokai.org/a/<token>>"
//!   /mypage                ────────► issue(player, Player) ─► DM "<https://dbrs.neokai.org/p/<token>>"
//!                                   └─ DMs closed ─► the link, only to them, here
//! ```
//!
//! The link sits in <…> so Discord shows no preview: its crawler never opens it. A new link replaces
//! the person's older one of the same kind.

use anyhow::{Context as _, Result};
use tracing::{info, warn};
use twilight_model::channel::message::AllowedMentions;
use twilight_model::id::Id;
use twilight_model::id::marker::UserMarker;

use super::ctx::Ctx;
use crate::services::links::Access;

const LASTS: &str = "Only for you. It works until 30 minutes pass without use, 4 hours at most. Don't share it.";

pub(crate) async fn admin(ctx: &Ctx<'_>) -> Result<()> {
    if !ctx.is_staff() {
        return ctx.deny().await;
    }
    let Some(staff) = ctx.user_id() else {
        return Ok(());
    };
    deliver(ctx, staff, Access::Staff, "🛠️ **Raging Spirit admin:**").await
}

pub(crate) async fn mypage(ctx: &Ctx<'_>) -> Result<()> {
    let Some(player) = ctx.user_id() else {
        return Ok(());
    };
    deliver(ctx, player, Access::Player, "🐉 **Your Raging Spirit page:** your characters, who you play, your Spirit Points.").await
}

async fn deliver(ctx: &Ctx<'_>, to: Id<UserMarker>, access: Access, heading: &str) -> Result<()> {
    ctx.defer().await?;
    let token = ctx.app.services.links().issue(to.get(), access)?;
    let kind = match access {
        Access::Staff => "a",
        Access::Player => "p",
    };
    let url = format!("{}/{kind}/{token}", ctx.app.web_url.trim_end_matches('/'));
    let text = format!("{heading}\n<{url}>\n{LASTS}");

    let dm = ctx.app.http.create_private_channel(to).await.context("opening a DM")?.model().await.context("reading the DM")?;
    let mentions = AllowedMentions::default();
    match ctx.app.http.create_message(dm.id).content(&text).allowed_mentions(Some(&mentions)).await {
        Ok(_) => {
            info!(?access, "web link sent by DM");
            ctx.edit("📬 Sent to your DMs.").await
        }
        Err(source) => {
            warn!(?source, ?access, "DMs closed; the web link is whispered instead");
            ctx.edit(&format!("Your DMs are closed, so here it is.\n{text}")).await
        }
    }
}
