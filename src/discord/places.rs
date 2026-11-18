//! Where a message was posted, and the webhook that posts as characters there.
//!
//! ```text
//!   spot(#chat)            ─► hook #chat, no thread, category Earth
//!   spot(a thread in #chat) ─► hook #chat (webhooks live on the parent), thread, category Earth
//!   hook(#chat)            ─► the bot's own webhook there, else a new one: "noxaDBRS proxy"
//! ```
//!
//! Both are kept: a channel's place for 10 minutes, a webhook for as long as it works.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use twilight_model::channel::ChannelType;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, WebhookMarker};

use super::app::App;

const SPOT_KEPT: Duration = Duration::from_secs(10 * 60);
const HOOK_NAME: &str = "noxaDBRS proxy";

/// Where a channel is.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Spot {
    /// The channel a webhook posts to: the channel itself, or a thread's parent.
    pub(crate) hook_channel: Id<ChannelMarker>,
    /// The thread, when the channel is one.
    pub(crate) thread: Option<Id<ChannelMarker>>,
    /// The category it sits in, through its parent for a thread.
    pub(crate) category: Option<u64>,
}

#[derive(Clone)]
pub(crate) struct Hook {
    pub(crate) id: Id<WebhookMarker>,
    pub(crate) token: String,
}

#[derive(Default)]
pub(crate) struct Places {
    spots: Mutex<HashMap<Id<ChannelMarker>, (Instant, Spot)>>,
    hooks: Mutex<HashMap<Id<ChannelMarker>, Hook>>,
}

const fn is_thread(kind: ChannelType) -> bool {
    matches!(kind, ChannelType::PublicThread | ChannelType::PrivateThread | ChannelType::AnnouncementThread)
}

pub(crate) async fn spot(app: &App, channel: Id<ChannelMarker>) -> Result<Spot> {
    let kept = app.places.spots.lock().unwrap_or_else(PoisonError::into_inner).get(&channel).filter(|(at, _)| at.elapsed() < SPOT_KEPT).map(|(_, s)| *s);
    if let Some(spot) = kept {
        return Ok(spot);
    }
    let found = app.http.channel(channel).await.context("reading a channel")?.model().await?;
    let spot = match (is_thread(found.kind), found.parent_id) {
        (true, Some(parent)) => {
            let parent_channel = app.http.channel(parent).await.context("reading a thread's parent")?.model().await?;
            Spot { hook_channel: parent, thread: Some(channel), category: parent_channel.parent_id.map(Id::get) }
        }
        _ => Spot { hook_channel: channel, thread: None, category: found.parent_id.map(Id::get) },
    };
    app.places.spots.lock().unwrap_or_else(PoisonError::into_inner).insert(channel, (Instant::now(), spot));
    Ok(spot)
}

/// The bot's webhook on a channel: one it made before, else a new one. Needs Manage Webhooks.
pub(crate) async fn hook(app: &App, channel: Id<ChannelMarker>) -> Result<Hook> {
    if let Some(hook) = app.places.hooks.lock().unwrap_or_else(PoisonError::into_inner).get(&channel).cloned() {
        return Ok(hook);
    }
    let existing = app.http.channel_webhooks(channel).await.context("listing the channel's webhooks: does the bot have Manage Webhooks?")?.models().await?;
    let own = existing.into_iter().find_map(|w| (w.application_id == Some(app.app_id)).then_some(()).and(w.token.map(|token| Hook { id: w.id, token })));
    let hook = match own {
        Some(hook) => hook,
        None => {
            let made = app.http.create_webhook(channel, HOOK_NAME).await.context("making a webhook: does the bot have Manage Webhooks?")?.model().await?;
            Hook { id: made.id, token: made.token.context("a new webhook without a token")? }
        }
    };
    app.places.hooks.lock().unwrap_or_else(PoisonError::into_inner).insert(channel, hook.clone());
    Ok(hook)
}

/// A webhook that stopped working (deleted by someone) is dropped, so the next post makes another.
pub(crate) fn forget_hook(app: &App, channel: Id<ChannelMarker>) {
    app.places.hooks.lock().unwrap_or_else(PoisonError::into_inner).remove(&channel);
}

/// Whether a channel is a roleplay one: inside a world.
pub(crate) async fn roleplay(app: &App, channel: Id<ChannelMarker>) -> Result<bool> {
    let Some(category) = spot(app, channel).await?.category else {
        return Ok(false);
    };
    Ok(app.services.worlds().all()?.contains(&category))
}
