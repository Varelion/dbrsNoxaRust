//! /tutorial [guide]: the interactive tutorial, Bot3's island in a thread of your own. One task at a
//! time; the bot moves on only once it sees the task done, wherever you do it.
//!
//! ```text
//!   /tutorial ─► a private thread "<name>'s tutorial" (public if private ones are not allowed, else DMs)
//!     **Step 1/10 · Your page**  Type `/mypage` …     [Show again] [Skip] [Stop]
//!     you type /mypage ─► "✅ Your page: done." ─► **Step 2/10 · Your slots** …
//!     the last step done ─► 🎓 finished; the thread stays to read back
//! ```
//!
//! What counts as done: domain::tutorial. Every handler reports what a person did through `observe`.

use anyhow::Result;
use tracing::{info, warn};
use twilight_model::application::interaction::message_component::MessageComponentInteractionData;
use twilight_model::channel::ChannelType;
use twilight_model::channel::message::AllowedMentions;
use twilight_model::channel::message::component::{ButtonStyle, Component};
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, UserMarker};

use super::app::App;
use super::ctx::{Ctx, Options};
use super::widgets::{button, row};
use crate::domain::tutorial::{Did, Step, Track};
use crate::services::tutorials::Moved;

/// The option that picks a track.
pub(crate) const GUIDE: &str = "guide";
const PREFIX: &str = "noxadbrs|tut|";
const AGAIN: &str = "again";
const SKIP: &str = "skip";
const STOP: &str = "stop";
const THREAD_NAME_MAX: usize = 90;
const FINISHED: &str = "🎓 **Tutorial finished.** That is everything you need. `/help` explains any command; `/tutorial` runs this again.";

fn controls(user: Id<UserMarker>) -> Vec<Component> {
    let id = |verb: &str| format!("{PREFIX}{verb}|{user}");
    vec![row(vec![
        button(id(AGAIN), "Show again", ButtonStyle::Secondary),
        button(id(SKIP), "Skip this step", ButtonStyle::Secondary),
        button(id(STOP), "Stop", ButtonStyle::Danger),
    ])]
}

fn step_text(track: Track, step: &Step) -> String {
    let (n, of) = track.place(step.id);
    format!("**Step {n}/{of} · {}**\n{}", step.title, step.task)
}

async fn say(app: &App, channel: Id<ChannelMarker>, text: &str, components: &[Component]) -> Result<()> {
    let mentions = AllowedMentions::default();
    app.http.create_message(channel).content(text).components(components).allowed_mentions(Some(&mentions)).await?;
    Ok(())
}

// A thread of their own: private if the bot may make one, else public, else their DMs.
async fn open(ctx: &Ctx<'_>, user: Id<UserMarker>, name: &str) -> Option<Id<ChannelMarker>> {
    let channel = ctx.channel_id()?;
    let title: String = format!("{name}'s tutorial").chars().take(THREAD_NAME_MAX).collect();
    for kind in [ChannelType::PrivateThread, ChannelType::PublicThread] {
        let Ok(made) = ctx.app.http.create_thread(channel, &title, kind).await else {
            continue;
        };
        let Ok(thread) = made.model().await else {
            continue;
        };
        ctx.app.http.add_thread_member(thread.id, user).await.ok();
        return Some(thread.id);
    }
    warn!("no thread for a tutorial here; it runs in DMs");
    let dm = ctx.app.http.create_private_channel(user).await.ok()?.model().await.ok()?;
    Some(dm.id)
}

pub(crate) async fn start(ctx: &Ctx<'_>, opts: &Options<'_>) -> Result<()> {
    let track = opts.string(GUIDE).and_then(Track::from_key).unwrap_or(Track::Players);
    if track == Track::Staff && !ctx.is_staff() {
        return ctx.deny().await;
    }
    let Some(user) = ctx.user_id() else {
        return Ok(());
    };
    ctx.defer().await?;
    let name = ctx.app.services.characters().active(user.get())?.map_or_else(|| "Your".to_owned(), |c| c.name);
    let Some(place) = open(ctx, user, &name).await else {
        return ctx.edit("I could not open a thread or a DM for your tutorial. Allow DMs from this server's members and try again.").await;
    };
    let at = ctx.app.services.tutorials().start(user.get(), track, Some(place.get()))?;
    info!(?track, "tutorial started");

    let none = track == Track::Players && ctx.app.services.characters().of_player(user.get())?.is_empty();
    let warning = if none { "\n-# You have no characters yet: the steps about them wait until staff add one, or press **Skip this step**." } else { "" };
    let intro = format!("👋 <@{user}>, one task at a time: do it, and I move on when I see it done.{warning}\n\n{}", step_text(track, at.step));
    say(ctx.app, place, &intro, &controls(user)).await?;
    ctx.edit(&format!("📘 Your tutorial is open in <#{place}>. Do each step there.")).await
}

// A step done: say so, then give the next one, or the end.
async fn announce(app: &App, user: u64, moved: Moved) {
    let (Some(place), Some(user)) = (moved.thread.and_then(Id::new_checked), Id::new_checked(user)) else {
        return;
    };
    let next = match moved.next {
        Some(step) => format!("✅ **{}**: done.\n\n{}", moved.done.title, step_text(moved.track, step)),
        None => format!("✅ **{}**: done.\n\n{FINISHED}", moved.done.title),
    };
    let components = if moved.next.is_some() { controls(user) } else { Vec::new() };
    if let Err(source) = say(app, place, &next, &components).await {
        warn!(?source, "the tutorial's next step could not be posted");
    }
}

/// Something someone did: the tutorial moves on if it was what their step waits for.
pub(crate) async fn observe(app: &App, user: u64, did: Did) {
    match app.services.tutorials().did(user, &did) {
        Ok(Some(moved)) => announce(app, user, moved).await,
        Ok(None) => {}
        Err(source) => warn!(?source, "reading the tutorial failed"),
    }
}

pub(crate) fn owns(custom_id: &str) -> bool {
    custom_id.starts_with(PREFIX)
}

/// Show again, skip, stop: for the tutorial's owner only.
pub(crate) async fn component(ctx: &Ctx<'_>, data: &MessageComponentInteractionData) -> Result<()> {
    let mut parts = data.custom_id.strip_prefix(PREFIX).unwrap_or_default().split('|');
    let (Some(verb), Some(owner)) = (parts.next(), parts.next().and_then(|o| o.parse::<u64>().ok())) else {
        return Ok(());
    };
    let Some(me) = ctx.user_id().map(Id::get) else {
        return Ok(());
    };
    if me != owner {
        return ctx.whisper("That is someone else's tutorial. `/tutorial` starts yours.").await;
    }
    let tutorials = ctx.app.services.tutorials();
    match verb {
        AGAIN => match tutorials.at(me)? {
            Some(at) => ctx.whisper(&step_text(at.track, at.step)).await,
            None => ctx.whisper("Your tutorial is over. `/tutorial` runs it again.").await,
        },
        SKIP => match tutorials.skip(me)? {
            Some(moved) => {
                ctx.whisper("Skipped.").await?;
                announce(ctx.app, me, moved).await;
                Ok(())
            }
            None => ctx.whisper("Your tutorial is over. `/tutorial` runs it again.").await,
        },
        STOP => {
            tutorials.stop(me)?;
            ctx.whisper("Tutorial stopped. `/tutorial` starts it over.").await
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_says_where_it_is_in_its_track() {
        let first = &Track::Players.steps()[0];
        assert!(step_text(Track::Players, first).starts_with(&format!("**Step 1/{} · Your page**", Track::Players.steps().len())));
    }
}
