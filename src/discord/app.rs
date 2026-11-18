//! What every handler shares: the HTTP client, the server, the services, the server's role names, and
//! what the web pages ask Discord for (names, channels, picture links), cached.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use twilight_gateway::Event;
use twilight_http::Client;
use twilight_model::gateway::payload::incoming::GuildCreate;
use twilight_model::id::Id;
use twilight_model::id::marker::{ApplicationMarker, ChannelMarker, GuildMarker, RoleMarker};

use super::guild::Caches;
use crate::services::Services;
use crate::telemetry::Stats;

pub(crate) struct App {
    pub(crate) http: Client,
    pub(crate) app_id: Id<ApplicationMarker>,
    pub(crate) guild_id: Id<GuildMarker>,
    pub(crate) services: Arc<Services>,
    pub(crate) stats: Arc<Stats>,
    /// Where the admin pages are opened from, for the links staff are sent.
    pub(crate) web_url: String,
    /// Where uploaded pictures are kept; None: uploads are off.
    pub(crate) pictures: Option<Id<ChannelMarker>>,
    pub(crate) caches: Caches,
    // Interactions carry role ids; staff roles are known by name. Filled by GuildCreate, kept by role events.
    roles: RwLock<HashMap<Id<RoleMarker>, String>>,
}

/// Where the bot is reached from, beside Discord: the admin pages, and the pictures channel.
pub(crate) struct Places {
    pub(crate) web_url: String,
    pub(crate) pictures: Option<Id<ChannelMarker>>,
}

impl App {
    pub(crate) fn new(http: Client, app_id: Id<ApplicationMarker>, guild_id: Id<GuildMarker>, services: Arc<Services>, places: Places) -> Self {
        Self {
            http,
            app_id,
            guild_id,
            services,
            stats: Arc::new(Stats::new()),
            web_url: places.web_url,
            pictures: places.pictures,
            caches: Caches::default(),
            roles: RwLock::new(HashMap::new()),
        }
    }

    /// Keeps the role names current; every other event passes through.
    pub(crate) fn observe(&self, event: &Event) {
        let mut roles = self.roles.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        match event {
            Event::GuildCreate(create) if create.id() == self.guild_id => {
                if let GuildCreate::Available(guild) = create.as_ref() {
                    *roles = guild.roles.iter().map(|role| (role.id, role.name.clone())).collect();
                }
            }
            Event::RoleCreate(e) if e.guild_id == self.guild_id => {
                roles.insert(e.role.id, e.role.name.clone());
            }
            Event::RoleUpdate(e) if e.guild_id == self.guild_id => {
                roles.insert(e.role.id, e.role.name.clone());
            }
            Event::RoleDelete(e) if e.guild_id == self.guild_id => {
                roles.remove(&e.role_id);
            }
            _ => {}
        }
    }

    pub(crate) fn role_names(&self, ids: &[Id<RoleMarker>]) -> Vec<String> {
        let roles = self.roles.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        ids.iter().filter_map(|id| roles.get(id).cloned()).collect()
    }
}
