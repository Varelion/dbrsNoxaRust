//! Where a message was posted, and the webhook that posts as characters there.
//!
//! ```text
//!   spot(#chat)            ─► hook #chat, no thread, category Earth
//!   spot(a thread in #chat) ─► hook #chat (webhooks live on the parent), thread, category Earth
//!   hook(#chat)            ─► the bot's own webhook there, else a new one: "noxaDBRS proxy"
//!   webhook(#chat, &BROLY) ─► the same, for another name: "Z Broly", with his face
//! ```
//!
//! Both are kept: a channel's place for 10 minutes, a webhook for as long as it works.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use twilight_model::channel::ChannelType;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, WebhookMarker};

use super::app::App;

const SPOT_KEPT: Duration = Duration::from_secs(10 * 60);

/// Who a webhook is: its name, and the face it is made with.
pub(crate) struct Persona {
    pub(crate) name: &'static str,
    pub(crate) jpeg: Option<&'static [u8]>,
}

const PROXY: Persona = Persona { name: "noxaDBRS proxy", jpeg: None };

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
    hooks: Mutex<HashMap<(Id<ChannelMarker>, &'static str), Hook>>,
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
    webhook(app, channel, &PROXY).await
}

/// The bot's webhook with this persona's name on a channel, else a new one with its face.
pub(crate) async fn webhook(app: &App, channel: Id<ChannelMarker>, persona: &Persona) -> Result<Hook> {
    let key = (channel, persona.name);
    if let Some(hook) = app.places.hooks.lock().unwrap_or_else(PoisonError::into_inner).get(&key).cloned() {
        return Ok(hook);
    }
    let existing = app.http.channel_webhooks(channel).await.context("listing the channel's webhooks: does the bot have Manage Webhooks?")?.models().await?;
    let own = existing
        .into_iter()
        .filter(|w| w.application_id == Some(app.app_id) && w.name.as_deref() == Some(persona.name))
        .find_map(|w| w.token.map(|token| Hook { id: w.id, token }));
    let hook = match own {
        Some(hook) => hook,
        None => {
            let face = persona.jpeg.map(|jpeg| format!("data:image/jpeg;base64,{}", STANDARD.encode(jpeg)));
            let mut create = app.http.create_webhook(channel, persona.name);
            if let Some(face) = face.as_deref() {
                create = create.avatar(face);
            }
            let made = create.await.context("making a webhook: does the bot have Manage Webhooks?")?.model().await?;
            Hook { id: made.id, token: made.token.context("a new webhook without a token")? }
        }
    };
    app.places.hooks.lock().unwrap_or_else(PoisonError::into_inner).insert(key, hook.clone());
    Ok(hook)
}

/// A webhook that stopped working (deleted by someone) is dropped, so the next post makes another.
pub(crate) fn forget_hook(app: &App, channel: Id<ChannelMarker>) {
    app.places.hooks.lock().unwrap_or_else(PoisonError::into_inner).remove(&(channel, PROXY.name));
}

/// Whether a channel is a roleplay one: inside a world.
pub(crate) async fn roleplay(app: &App, channel: Id<ChannelMarker>) -> Result<bool> {
    let Some(category) = spot(app, channel).await?.category else {
        return Ok(false);
    };
    Ok(app.services.worlds().all()?.contains(&category))
}
