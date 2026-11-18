//! The Discord side of `services::guild`: answers what the web pages ask for.
//!
//! ```text
//!   Ask::Audit    ─► #mod-log, one line at a time, in order
//!   Ask::People   ─► the member list (one call, kept 10 minutes), else one call per unknown id
//!   Ask::Channels ─► the server's channels, kept 1 minute
//!   Ask::Upload   ─► a message in the pictures channel, the image attached
//!   Ask::Fresh    ─► each picture's link; one near its expiry is fetched again from its message
//! ```
//!
//! Everything but Audit runs in its own task, so a slow upload never holds up a mod log line.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::{error, info, warn};
use twilight_model::channel::ChannelType;
use twilight_model::channel::message::AllowedMentions;
use twilight_model::guild::Member;
use twilight_model::http::attachment::Attachment;
use twilight_model::id::Id;

use super::app::App;
use super::modlog;
use super::profile::icon_url;
use crate::domain::pictures::expires_at;
use crate::services::guild::{Ask, Channel, ChannelKind, Person, Stored, Upload, UploadRefused, Uploaded};
use crate::store::characters::CharacterRow;

const NAMES_KEPT: Duration = Duration::from_secs(10 * 60);
const CHANNELS_KEPT: Duration = Duration::from_secs(60);
// Discord hands out at most this many members per call.
const MEMBER_PAGE: u16 = 1000;
// Ids looked up one by one per ask, when the member list is not allowed: each is a call.
const LOOKUPS_MAX: usize = 20;
// A link this close to expiring is fetched again: a page may stay open a while.
const FRESH_MARGIN_S: u64 = 60 * 60;
const ATTACHMENT_ID: u64 = 1;

#[derive(Default)]
struct Names {
    fetched: Option<Instant>,
    known: HashMap<u64, Person>,
    // Ids Discord did not know this round: not asked again until the list is refetched.
    missed: HashSet<u64>,
}

/// What the web pages ask for often, kept a while.
#[derive(Default)]
pub(crate) struct Caches {
    names: Mutex<Names>,
    channels: Mutex<Option<(Instant, Vec<Channel>)>>,
    links: Mutex<HashMap<u64, String>>,
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) async fn answer(app: Arc<App>, mut asks: UnboundedReceiver<Ask>) {
    while let Some(ask) = asks.recv().await {
        let app = Arc::clone(&app);
        match ask {
            Ask::Audit(line) => audit(&app, &line).await,
            Ask::People { users, reply } => {
                tokio::spawn(async move { reply.send(people(&app, &users).await).ok() });
            }
            Ask::Channels { reply } => {
                tokio::spawn(async move { reply.send(channels(&app).await).ok() });
            }
            Ask::Upload { upload, reply } => {
                tokio::spawn(async move { reply.send(self::upload(&app, upload).await).ok() });
            }
            Ask::Fresh { pictures, reply } => {
                tokio::spawn(async move { reply.send(fresh(&app, pictures).await).ok() });
            }
        }
    }
}

// A web change's line, posted to the mod log like any other.
async fn audit(app: &App, line: &str) {
    match modlog::required(app) {
        Ok(Some(channel)) => {
            modlog::post(app, channel, line).await;
        }
        Ok(None) => warn!(target: "audit", %line, "no mod log for a web change; it stays in this journal only"),
        Err(source) => error!(target: "audit", ?source, %line, "reading the mod log setting failed"),
    }
}

fn person_of(app: &App, member: &Member) -> Person {
    let name = member.nick.clone().or_else(|| member.user.global_name.clone()).unwrap_or_else(|| member.user.name.clone());
    Person { name, icon: icon_url(app.guild_id, member.user.id, member.avatar, member.user.avatar) }
}

async fn member_list(app: &App) -> Result<HashMap<u64, Person>> {
    let mut all = HashMap::new();
    let mut after = None;
    loop {
        let mut request = app.http.guild_members(app.guild_id).limit(MEMBER_PAGE);
        if let Some(last) = after {
            request = request.after(last);
        }
        let page = request.await?.models().await?;
        all.extend(page.iter().map(|m| (m.user.id.get(), person_of(app, m))));
        if page.len() < usize::from(MEMBER_PAGE) {
            return Ok(all);
        }
        after = page.last().map(|m| m.user.id);
    }
}

async fn people(app: &App, users: &[u64]) -> HashMap<u64, Person> {
    let stale = locked(&app.caches.names).fetched.is_none_or(|at| at.elapsed() > NAMES_KEPT);
    if stale {
        // Listing members needs the Server Members intent; without it, ids are looked up one by one.
        let listed = member_list(app).await;
        let mut names = locked(&app.caches.names);
        match listed {
            Ok(all) => names.known = all,
            Err(source) => warn!(?source, "the member list is not allowed; names are looked up one by one"),
        }
        names.fetched = Some(Instant::now());
        names.missed.clear();
    }

    let unknown: Vec<u64> = {
        let names = locked(&app.caches.names);
        users.iter().copied().filter(|id| !names.known.contains_key(id) && !names.missed.contains(id)).take(LOOKUPS_MAX).collect()
    };
    for id in unknown {
        let Some(user) = Id::new_checked(id) else {
            continue;
        };
        let found = match app.http.guild_member(app.guild_id, user).await {
            Ok(response) => response.model().await.ok().map(|m| person_of(app, &m)),
            Err(_) => None,
        };
        let mut names = locked(&app.caches.names);
        match found {
            Some(person) => {
                names.known.insert(id, person);
            }
            None => {
                names.missed.insert(id);
            }
        }
    }

    let names = locked(&app.caches.names);
    users.iter().filter_map(|id| Some((*id, names.known.get(id)?.clone()))).collect()
}

const fn kind_of(kind: ChannelType) -> ChannelKind {
    match kind {
        ChannelType::GuildCategory => ChannelKind::Category,
        ChannelType::GuildText => ChannelKind::Text,
        _ => ChannelKind::Other,
    }
}

async fn channels(app: &App) -> Vec<Channel> {
    let kept = locked(&app.caches.channels).as_ref().filter(|(at, _)| at.elapsed() < CHANNELS_KEPT).map(|(_, kept)| kept.clone());
    if let Some(kept) = kept {
        return kept;
    }
    let fetched = match app.http.guild_channels(app.guild_id).await {
        Ok(response) => response.models().await.map_err(anyhow::Error::from),
        Err(source) => Err(source.into()),
    };
    let all = match fetched {
        Ok(all) => all,
        Err(source) => {
            warn!(?source, "reading the server's channels failed");
            return Vec::new();
        }
    };
    let mut channels: Vec<Channel> = all
        .into_iter()
        .map(|c| Channel {
            id: c.id.get(),
            name: c.name.unwrap_or_default(),
            kind: kind_of(c.kind),
            parent: c.parent_id.map(Id::get),
            position: c.position.unwrap_or_default(),
        })
        .collect();
    channels.sort_by_key(|c| (c.position, c.id));
    *locked(&app.caches.channels) = Some((Instant::now(), channels.clone()));
    channels
}

async fn upload(app: &App, upload: Upload) -> Result<Uploaded, UploadRefused> {
    let Some(channel) = app.pictures else {
        return Err(UploadRefused::NoChannel);
    };
    let size = upload.bytes.len();
    let files = [Attachment::from_bytes(format!("{}.{}", upload.stem, upload.kind.extension()), upload.bytes, ATTACHMENT_ID)];
    let mentions = AllowedMentions::default();
    let sent = app.http.create_message(channel).content(&upload.caption).allowed_mentions(Some(&mentions)).attachments(&files).await;
    let message = match sent {
        Ok(response) => response.model().await.map_err(|source| {
            error!(?source, "reading the uploaded picture's message failed");
            UploadRefused::Refused
        })?,
        Err(source) => {
            error!(?source, %channel, size, "the pictures channel refused the upload");
            return Err(UploadRefused::Refused);
        }
    };
    let Some(file) = message.attachments.first() else {
        error!(message = %message.id, "the uploaded picture's message has no attachment");
        return Err(UploadRefused::Refused);
    };
    info!(message = %message.id, size, "picture uploaded");
    locked(&app.caches.links).insert(message.id.get(), file.url.clone());
    Ok(Uploaded { message_id: message.id.get(), url: file.url.clone() })
}

fn now_s() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

// The picture's link from its message, as Discord signs it now.
async fn refetched(app: &App, message_id: u64) -> Option<String> {
    let (channel, message) = (app.pictures?, Id::new_checked(message_id)?);
    let found = match app.http.message(channel, message).await {
        Ok(response) => response.model().await.ok(),
        Err(source) => {
            warn!(?source, message_id, "an uploaded picture's message is gone or unreadable");
            None
        }
    };
    let url = found?.attachments.first()?.url.clone();
    locked(&app.caches.links).insert(message_id, url.clone());
    Some(url)
}

async fn fresh(app: &App, pictures: Vec<Stored>) -> HashMap<u64, String> {
    let mut links = HashMap::new();
    for picture in pictures {
        let known = locked(&app.caches.links).get(&picture.message_id).cloned().unwrap_or(picture.url);
        let lasting = expires_at(&known).is_none_or(|at| at > now_s() + FRESH_MARGIN_S);
        let link = if lasting { Some(known) } else { refetched(app, picture.message_id).await };
        if let Some(link) = link {
            links.insert(picture.message_id, link);
        }
    }
    links
}

/// A character's picture for a card: a fresh link for an upload, else its own link; None: their icon.
pub(crate) async fn picture_of(app: &App, row: &CharacterRow) -> Option<String> {
    let Some(stored) = Stored::of(row) else {
        return row.picture_url.clone();
    };
    let message_id = stored.message_id;
    fresh(app, vec![stored]).await.remove(&message_id).or_else(|| row.picture_url.clone())
}
