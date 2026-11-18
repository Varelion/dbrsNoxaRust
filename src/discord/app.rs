//! What every handler shares: the HTTP client, the server, the services, the server's role names, and
//! what the web pages ask Discord for (names, channels, picture links), cached.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use twilight_gateway::Event;
use twilight_http::Client;
use twilight_model::gateway::payload::incoming::GuildCreate;
use twilight_model::guild::Permissions;
use twilight_model::id::Id;
use twilight_model::id::marker::{ApplicationMarker, ChannelMarker, GuildMarker, RoleMarker, UserMarker};

use super::guild::Caches;
use crate::services::Services;
use crate::services::authority::{Caller, can_moderate};
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
    pub(crate) reading: Reading,
    // Interactions carry role ids; staff roles are known by name. Filled by GuildCreate, kept by role events.
    roles: RwLock<HashMap<Id<RoleMarker>, Role>>,
    // The server's owner holds every power; a message carries no permissions, only roles.
    owner: RwLock<Option<Id<UserMarker>>>,
}

struct Role {
    name: String,
    permissions: Permissions,
}

/// Whether the bot reads message text: dot commands need the Message Content intent, which the
/// Developer Portal enables, not the bot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reading {
    Messages,
    CommandsOnly,
}

/// Where the bot is reached from, beside Discord: the admin pages, and the pictures channel.
pub(crate) struct Places {
    pub(crate) web_url: String,
    pub(crate) pictures: Option<Id<ChannelMarker>>,
}

impl App {
    pub(crate) fn new(http: Client, app_id: Id<ApplicationMarker>, guild_id: Id<GuildMarker>, services: Arc<Services>, places: Places, reading: Reading) -> Self {
        Self {
            http,
            app_id,
            guild_id,
            services,
            stats: Arc::new(Stats::new()),
            web_url: places.web_url,
            pictures: places.pictures,
            caches: Caches::default(),
            reading,
            roles: RwLock::new(HashMap::new()),
            owner: RwLock::new(None),
        }
    }

    /// Keeps the roles and the owner current; every other event passes through.
    pub(crate) fn observe(&self, event: &Event) {
        let mut roles = self.roles.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        let role = |r: &twilight_model::guild::Role| Role { name: r.name.clone(), permissions: r.permissions };
        match event {
            Event::GuildCreate(create) if create.id() == self.guild_id => {
                if let GuildCreate::Available(guild) = create.as_ref() {
                    *roles = guild.roles.iter().map(|r| (r.id, role(r))).collect();
                    *self.owner.write().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(guild.owner_id);
                }
            }
            Event::RoleCreate(e) if e.guild_id == self.guild_id => {
                roles.insert(e.role.id, role(&e.role));
            }
            Event::RoleUpdate(e) if e.guild_id == self.guild_id => {
                roles.insert(e.role.id, role(&e.role));
            }
            Event::RoleDelete(e) if e.guild_id == self.guild_id => {
                roles.remove(&e.role_id);
            }
            _ => {}
        }
    }

    pub(crate) fn role_names(&self, ids: &[Id<RoleMarker>]) -> Vec<String> {
        let roles = self.roles.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        ids.iter().filter_map(|id| roles.get(id).map(|r| r.name.clone())).collect()
    }

    /// Staff, for a message's author: a message carries roles, not permissions, so they are summed here
    /// from the roles held and @everyone's (whose id is the server's).
    pub(crate) fn is_staff_member(&self, user: Id<UserMarker>, held: &[Id<RoleMarker>]) -> bool {
        if *self.owner.read().unwrap_or_else(std::sync::PoisonError::into_inner) == Some(user) {
            return true;
        }
        let everyone = self.guild_id.cast::<RoleMarker>();
        let powers = {
            let roles = self.roles.read().unwrap_or_else(std::sync::PoisonError::into_inner);
            held.iter().chain([&everyone]).filter_map(|id| roles.get(id)).fold(Permissions::empty(), |all, r| all | r.permissions)
        };
        let role_names = self.role_names(held);
        can_moderate(&Caller {
            administrator: powers.contains(Permissions::ADMINISTRATOR),
            manages_server: powers.contains(Permissions::MANAGE_GUILD),
            role_names: &role_names,
        })
    }
}
