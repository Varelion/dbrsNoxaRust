//! Dot commands, typed as a message: `.prune <count>`. Any other message is left alone.
//!
//! ```text
//!   .prune 10 ─ staff? ─ mod log set? ─► the 10 messages before it deleted, then the command itself
//!                                           older than 14 days: left, Discord bulk-deletes none of those
//!                                        ─► "🧹 Deleted 10 messages." for 5 seconds · a mod log line
//! ```
//!
//! Needs the Message Content intent, enabled in the Developer Portal: without it the bot reads no
//! message text, and dot commands are off (discord::run says so at start).

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use tracing::{info, warn};
use twilight_model::channel::Message;
use twilight_model::channel::message::AllowedMentions;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, MessageMarker};

use super::app::App;
use super::ctx::STAFF_ONLY;
use super::modlog::{self, NO_MOD_LOG};

pub(crate) const PRUNE: &str = ".prune";
pub(crate) const PRUNE_MAX: u16 = 100;
const USAGE: &str = "Usage: `.prune <count>`, 1 to 100: `.prune 10` deletes the 10 messages before it.";
const CANNOT_DELETE: &str = "I cannot delete messages here: give me Manage Messages and Read Message History.";
const SHOWN_FOR: Duration = Duration::from_secs(5);
// Discord bulk-deletes only messages younger than this, two at least, a hundred at most.
const BULK_AGE_MS: u64 = 14 * 24 * 60 * 60 * 1000;
const BULK_MIN: usize = 2;
// Snowflakes count milliseconds from Discord's epoch, in their top 42 bits.
const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;
const SNOWFLAKE_TIME_SHIFT: u32 = 22;

/// What a message asks for, if it is a dot command.
#[derive(Debug, PartialEq, Eq)]
enum Dot {
    Prune(Option<u16>),
}

impl Dot {
    // ".prune 10", ".PRUNE 10": the command; "pruned it" or a word after it: not one.
    fn of(content: &str) -> Option<Self> {
        let mut words = content.split_whitespace();
        if !words.next()?.eq_ignore_ascii_case(PRUNE) {
            return None;
        }
        let count = words.next().and_then(|w| w.parse::<u16>().ok()).filter(|n| (1..=PRUNE_MAX).contains(n));
        Some(Self::Prune(count))
    }
}

/// When a message was sent, in milliseconds since the epoch, from its id.
const fn sent_at(id: Id<MessageMarker>) -> u64 {
    (id.get() >> SNOWFLAKE_TIME_SHIFT) + DISCORD_EPOCH_MS
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
}

/// A message's dot command, run; any other message is ignored.
pub(crate) async fn handle(app: Arc<App>, message: Message) -> Result<()> {
    if message.author.bot || message.guild_id != Some(app.guild_id) {
        return Ok(());
    }
    let Some(dot) = Dot::of(&message.content) else {
        return Ok(());
    };
    match dot {
        Dot::Prune(count) => prune(&app, &message, count).await,
    }
}

// A short answer in the channel, no pings, gone after a few seconds so the channel stays clean.
async fn say(app: &Arc<App>, channel: Id<ChannelMarker>, text: &str) -> Result<()> {
    let mentions = AllowedMentions::default();
    let sent = app.http.create_message(channel).content(text).allowed_mentions(Some(&mentions)).await?.model().await?;
    let (app, id) = (Arc::clone(app), sent.id);
    tokio::spawn(async move {
        tokio::time::sleep(SHOWN_FOR).await;
        app.http.delete_message(channel, id).await.ok();
    });
    Ok(())
}

async fn prune(app: &Arc<App>, message: &Message, count: Option<u16>) -> Result<()> {
    let channel = message.channel_id;
    let roles = message.member.as_ref().map(|m| m.roles.as_slice()).unwrap_or_default();
    if !app.is_staff_member(message.author.id, roles) {
        info!(user = %message.author.id, "refused: .prune, not staff");
        return say(app, channel, STAFF_ONLY).await;
    }
    let Some(count) = count else {
        return say(app, channel, USAGE).await;
    };
    let Some(log) = modlog::required(app)? else {
        return say(app, channel, NO_MOD_LOG).await;
    };

    let before = match app.http.channel_messages(channel).limit(count).before(message.id).await {
        Ok(response) => response.models().await?,
        Err(source) => {
            warn!(?source, %channel, "reading messages to prune failed");
            return say(app, channel, CANNOT_DELETE).await;
        }
    };
    let oldest_allowed = now_ms().saturating_sub(BULK_AGE_MS);
    let (young, old): (Vec<_>, Vec<_>) = before.iter().map(|m| m.id).partition(|id| sent_at(*id) > oldest_allowed);

    let deleted = match young.len() {
        0 => Ok(()),
        n if n < BULK_MIN => app.http.delete_message(channel, young[0]).await.map(|_| ()),
        _ => app.http.delete_messages(channel, &young).await.map(|_| ()),
    };
    if let Err(source) = deleted {
        warn!(?source, %channel, "pruning failed");
        return say(app, channel, CANNOT_DELETE).await;
    }
    app.http.delete_message(channel, message.id).await.ok();

    let staff = message.author.id;
    let left = if old.is_empty() { String::new() } else { format!(" {} older than 14 days left: Discord does not bulk-delete those.", old.len()) };
    modlog::post(app, log, &format!("🧹 <@{staff}> pruned **{}** messages in <#{channel}>", young.len())).await;
    say(app, channel, &format!("🧹 Deleted {} messages.{left}", young.len())).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_takes_a_count_from_1_to_100() {
        assert_eq!(Dot::of(".prune 10"), Some(Dot::Prune(Some(10))));
        assert_eq!(Dot::of("  .PRUNE   100 "), Some(Dot::Prune(Some(100))));
        assert_eq!(Dot::of(".prune"), Some(Dot::Prune(None)));
        assert_eq!(Dot::of(".prune 0"), Some(Dot::Prune(None)));
        assert_eq!(Dot::of(".prune 101"), Some(Dot::Prune(None)));
        assert_eq!(Dot::of(".prune ten"), Some(Dot::Prune(None)));
        assert_eq!(Dot::of(".pruned 10"), None);
        assert_eq!(Dot::of("he said .prune 10"), None);
        assert_eq!(Dot::of(""), None);
    }

    #[test]
    fn a_message_knows_when_it_was_sent() {
        // Discord's documented example: 175928847299117063 was sent at 1462015105796.
        assert_eq!(sent_at(Id::new(175_928_847_299_117_063)), 1_462_015_105_796);
    }
}
