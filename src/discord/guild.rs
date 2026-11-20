//! The Discord side of `services::guild`: answers what the web pages ask for.
//!
//! ```text
//!   Ask::Audit    ─► #mod-log, one line at a time, in order
//!   Ask::People   ─► the member list (one call, kept 10 minutes), else one call per unknown id
//!   Ask::Channels ─► the server's channels, kept 1 minute
//!   Ask::Fresh    ─► each Discord link near its expiry signed again (POST attachments/refresh-urls)
//! ```
//!
//! Everything but Audit runs in its own task, so a slow call never holds up a mod log line.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::{error, warn};
use twilight_model::channel::ChannelType;
use twilight_model::guild::Member;
use serde::{Deserialize, Serialize};
use twilight_http::request::{Method, RequestBuilder};
use twilight_model::id::Id;

use super::app::App;
use super::modlog;
use super::profile::icon_url;
use crate::domain::pictures::expires_at;
use crate::services::guild::{Ask, Channel, ChannelKind, Person, fresh_or};
use crate::store::characters::CharacterRow;

const NAMES_KEPT: Duration = Duration::from_secs(10 * 60);
const CHANNELS_KEPT: Duration = Duration::from_secs(60);
// Discord hands out at most this many members per call.
const MEMBER_PAGE: u16 = 1000;
// Ids looked up one by one per ask, when the member list is not allowed: each is a call.
const LOOKUPS_MAX: usize = 20;
// A link this close to expiring is fetched again: a page may stay open a while.
const FRESH_MARGIN_S: u64 = 60 * 60;
const REFRESH_PATH: &str = "attachments/refresh-urls";
// Discord signs at most this many links per call.
const REFRESH_BATCH: usize = 50;

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
    /// Discord links signed again: the stored link, and its fresh one.
    links: Mutex<HashMap<String, String>>,
    cooldowns: Mutex<HashMap<u64, Instant>>,
}

impl Caches {
    /// True when the user may act again: their last action was at least `gap` ago.
    pub(crate) fn cool(&self, user: u64, gap: Duration) -> bool {
        let mut seen = locked(&self.cooldowns);
        let now = Instant::now();
        if seen.get(&user).is_some_and(|at| now.duration_since(*at) < gap) {
            return false;
        }
        seen.insert(user, now);
        true
    }
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
            Ask::Fresh { links, reply } => {
                tokio::spawn(async move { reply.send(fresh(&app, links).await).ok() });
            }
            Ask::Did { user, did } => {
                tokio::spawn(async move { super::tutorial::observe(&app, user, did).await });
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

fn now_s() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

#[derive(Serialize)]
struct RefreshAsk<'a> {
    attachment_urls: &'a [String],
}

#[derive(Deserialize)]
struct Refreshed {
    refreshed_urls: Vec<Pair>,
}

#[derive(Deserialize)]
struct Pair {
    original: String,
    refreshed: String,
}

// Discord's links signed again, as it answers: the link asked for, and its fresh one.
async fn refreshed(app: &App, stale: &[String]) -> Result<Vec<Pair>> {
    let body = serde_json::to_vec(&RefreshAsk { attachment_urls: stale })?;
    let request = RequestBuilder::raw(Method::Post, REFRESH_PATH.to_owned()).body(body).build()?;
    Ok(app.http.request::<Refreshed>(request).await?.model().await?.refreshed_urls)
}

// Each link, fresh: a lasting one as it is, a stale one signed again. One Discord cannot sign is left out.
async fn fresh(app: &App, links: Vec<String>) -> HashMap<String, String> {
    let mut fresh = HashMap::new();
    let mut stale = Vec::new();
    for link in links {
        if expires_at(&link).is_none() {
            continue;
        }
        let known = locked(&app.caches.links).get(&link).cloned().unwrap_or_else(|| link.clone());
        if expires_at(&known).is_some_and(|at| at > now_s() + FRESH_MARGIN_S) {
            fresh.insert(link, known);
            continue;
        }
        if !stale.contains(&link) {
            stale.push(link);
        }
    }
    for batch in stale.chunks(REFRESH_BATCH) {
        let pairs = match refreshed(app, batch).await {
            Ok(pairs) => pairs,
            Err(source) => {
                warn!(?source, count = batch.len(), "Discord did not sign links again");
                continue;
            }
        };
        let mut known = locked(&app.caches.links);
        for pair in pairs {
            known.insert(pair.original.clone(), pair.refreshed.clone());
            fresh.insert(pair.original, pair.refreshed);
        }
    }
    fresh
}

/// A link that works now: a stale Discord link signed again, anything else as it is.
pub(crate) async fn fresh_link(app: &App, url: &str) -> String {
    fresh_or(url, &fresh(app, vec![url.to_owned()]).await)
}

/// A character's picture, its link working now; None: their icon.
pub(crate) async fn picture_of(app: &App, row: &CharacterRow) -> Option<String> {
    let url = row.picture_url.as_deref()?;
    Some(fresh_link(app, url).await)
}
