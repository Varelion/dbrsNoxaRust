//! Spirit Points: staff change them, a player reads their own.
//!
//! ```text
//!   /z_moderator_sp action:give|take|set player amount memo   one player
//!   /z_moderator_sp action:grant role amount memo             everyone holding the role
//!   /z_moderator_sp action:history player                     the last lines of their ledger
//!
//! A player's own balance and lines are on their /me page.
//! ```
//!
//! Every change waits for the mod log and lands there.

use anyhow::Result;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, RoleMarker, UserMarker};

use super::commands::HISTORY;
use super::ctx::{Ctx, Options};
use super::modlog::{self, NO_MOD_LOG};
use crate::domain::spirit::{SP_MAX, SpAction};
use crate::services::spirit::{Change, Changed, Granted};
use crate::store::spirit::LedgerRow;

const SP: &str = "SP";
const HISTORY_LINES: usize = 10;
// Discord hands out at most this many members per request.
const MEMBER_PAGE: u16 = 1000;
const MS_PER_SECOND: i64 = 1000;

fn ledger_line(line: &LedgerRow) -> String {
    let sign = if line.delta > 0 { "+" } else { "" };
    format!("<t:{}:d> · **{sign}{}** → {} · {}", line.at / MS_PER_SECOND, line.delta, line.balance, line.memo)
}

pub(crate) async fn admin(ctx: &Ctx<'_>, opts: &Options<'_>) -> Result<()> {
    if !ctx.is_staff() {
        return ctx.deny().await;
    }
    ctx.defer().await?;

    let sub = opts.sub().unwrap_or_default();
    if sub == HISTORY {
        return history(ctx, opts).await;
    }
    let Some(log) = modlog::required(ctx.app)? else {
        return ctx.edit(NO_MOD_LOG).await;
    };
    let Some(staff) = ctx.user_id() else {
        return Ok(());
    };
    let Some(action) = SpAction::from_key(sub) else {
        return ctx.edit("Pick an action.").await;
    };
    if action == SpAction::Grant {
        return grant(ctx, opts, log, staff).await;
    }
    change_one(ctx, opts, log, action, staff).await
}

async fn change_one(ctx: &Ctx<'_>, opts: &Options<'_>, log: Id<ChannelMarker>, action: SpAction, staff: Id<UserMarker>) -> Result<()> {
    let (Some(player), Some(amount)) = (opts.user("player"), opts.integer("amount")) else {
        return ctx.edit("Name a player and an amount.").await;
    };
    let memo = opts.string("memo").unwrap_or_default();
    let change = Change { action, player_id: player.get(), amount, memo, staff_id: staff.get() };

    let (before, after) = match ctx.app.services.spirit().change(&change)? {
        Changed::Done { before, after } => (before, after),
        Changed::NoMemo => return ctx.edit("Write a memo: why.").await,
        Changed::BadAmount => return ctx.edit(&format!("Amount: a whole number from 1 to {SP_MAX} (set: from 0).")).await,
        Changed::TooLow { before } => {
            return ctx.edit(&format!("<@{player}> has {before} {SP}. Take {before} or less, or use set.")).await;
        }
    };

    let what = match action {
        SpAction::Take => format!("took **{amount}** {SP} from <@{player}>: {before} → **{after}**"),
        SpAction::Set => format!("set <@{player}>'s {SP} to **{after}** (was {before})"),
        SpAction::Give | SpAction::Grant => format!("gave **{amount}** {SP} to <@{player}>: {before} → **{after}**"),
    };
    let line = format!("🔮 <@{staff}> {what} · {}{}", memo.trim(), modlog::where_of(ctx.channel_id()));
    modlog::post(ctx.app, log, &line).await;
    ctx.edit(&format!("<@{player}>: {before} → **{after}** {SP}.")).await
}

// The players holding a role, bots left out, read page by page from Discord.
async fn holders(ctx: &Ctx<'_>, role: Id<RoleMarker>) -> Result<Vec<u64>> {
    let mut ids = Vec::new();
    let mut after = None;
    loop {
        let mut request = ctx.app.http.guild_members(ctx.app.guild_id).limit(MEMBER_PAGE);
        if let Some(last) = after {
            request = request.after(last);
        }
        let page = request.await?.models().await?;
        ids.extend(page.iter().filter(|m| !m.user.bot && m.roles.contains(&role)).map(|m| m.user.id.get()));
        if page.len() < usize::from(MEMBER_PAGE) {
            return Ok(ids);
        }
        after = page.last().map(|m| m.user.id);
    }
}

async fn grant(ctx: &Ctx<'_>, opts: &Options<'_>, log: Id<ChannelMarker>, staff: Id<UserMarker>) -> Result<()> {
    let (Some(role), Some(amount)) = (opts.role("role"), opts.integer("amount")) else {
        return ctx.edit("Name a role and an amount.").await;
    };
    let memo = opts.string("memo").unwrap_or_default();
    let players = holders(ctx, role).await?;

    let count = match ctx.app.services.spirit().grant(&players, amount, memo, staff.get())? {
        Granted::Done { count } => count,
        Granted::NoMemo => return ctx.edit("Write a memo: why.").await,
        Granted::BadAmount => return ctx.edit(&format!("Amount: a whole number from 1 to {SP_MAX}.")).await,
        Granted::NoPlayers => return ctx.edit("Nobody holds that role.").await,
    };

    let line = format!("🔮 <@{staff}> granted **{amount}** {SP} to **{count}** holders of <@&{role}> · {}{}", memo.trim(), modlog::where_of(ctx.channel_id()));
    modlog::post(ctx.app, log, &line).await;
    ctx.edit(&format!("**{count}** holders of <@&{role}> got **{amount}** {SP} each.")).await
}

async fn history(ctx: &Ctx<'_>, opts: &Options<'_>) -> Result<()> {
    let Some(player) = opts.user("player") else {
        return ctx.edit("Name a player.").await;
    };
    let spirit = ctx.app.services.spirit();
    let lines = spirit.history(player.get(), HISTORY_LINES)?;
    let body = if lines.is_empty() {
        "No ledger lines yet.".to_owned()
    } else {
        lines.iter().map(|l| format!("{} · <@{}>", ledger_line(l), l.staff_id)).collect::<Vec<_>>().join("\n")
    };
    ctx.edit(&format!("<@{player}>: **{}** {SP}\n{body}", spirit.balance(player.get())?)).await
}
