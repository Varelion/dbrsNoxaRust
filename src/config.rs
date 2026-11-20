//! Settings from the environment, or a .env file beside where noxaDBRS starts.
//!
//! ```text
//!   DISCORD_TOKEN     the bot's token
//!   GUILD_ID          the one server noxaDBRS serves
//!   NOXADBRS_DB       the database file; default noxadbrs.db
//!   WEB_BIND          where the pages also listen, plain HTTP on this machine; default 127.0.0.1:8787,
//!                     none with a relay unless set
//!   WEB_URL           the address links point at, e.g. https://dbrs.neokai.org; default http://WEB_BIND
//!   RELAY_URL         the neoKai relay to dial out to, wss://dbrs.neokai.org/relay; unset: no relay
//!   RELAY_SECRET      the relay's shared secret; needed with RELAY_URL
//! ```

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

const DEFAULT_DB: &str = "noxadbrs.db";
const DEFAULT_WEB_BIND: &str = "127.0.0.1:8787";
const SECURE_RELAY: &str = "wss://";
const SECURE_WEB: &str = "https://";

/// The neoKai relay: where to dial, and the secret it checks.
pub(crate) struct Relay {
    pub(crate) url: String,
    pub(crate) secret: String,
}

pub(crate) struct Config {
    pub(crate) token: String,
    pub(crate) guild_id: Id<GuildMarker>,
    pub(crate) db_path: PathBuf,
    pub(crate) web_bind: Option<SocketAddr>,
    pub(crate) web_url: String,
    pub(crate) relay: Option<Relay>,
}

// A variable that may be unset or blank: None either way.
fn optional(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.trim().is_empty())
}

impl Config {
    pub(crate) fn from_env() -> Result<Self> {
        // A missing .env is fine: the variables may come from systemd instead.
        dotenvy::dotenv().ok();

        let token = std::env::var("DISCORD_TOKEN").context("DISCORD_TOKEN is not set")?;
        let guild = std::env::var("GUILD_ID").context("GUILD_ID is not set")?;
        let guild_id = guild.parse::<u64>().ok().and_then(Id::new_checked).context("GUILD_ID is not a server id")?;
        let db_path = std::env::var("NOXADBRS_DB").map_or_else(|_| PathBuf::from(DEFAULT_DB), PathBuf::from);
        let relay = match (optional("RELAY_URL"), optional("RELAY_SECRET")) {
            (Some(url), _) if !url.starts_with(SECURE_RELAY) => bail!("RELAY_URL must start with {SECURE_RELAY}: the secret never travels unencrypted"),
            (Some(url), Some(secret)) => Some(Relay { url, secret }),
            (Some(_), None) => bail!("RELAY_URL is set without RELAY_SECRET"),
            (None, _) => None,
        };

        // Plain HTTP on this machine: on by default only without a relay, which is the way in otherwise.
        let bind = optional("WEB_BIND").or_else(|| relay.is_none().then(|| DEFAULT_WEB_BIND.to_owned()));
        let web_bind = bind.map(|b| b.parse().with_context(|| format!("WEB_BIND {b:?} is not host:port"))).transpose()?;
        let local = web_bind.map_or_else(|| DEFAULT_WEB_BIND.to_owned(), |b: SocketAddr| b.to_string());
        let web_url = optional("WEB_URL").unwrap_or_else(|| format!("http://{local}"));
        // Links carry a login token: through a relay they go out over https only.
        if relay.is_some() && !web_url.starts_with(SECURE_WEB) {
            bail!("WEB_URL must start with {SECURE_WEB} with a relay: links carry a login token");
        }
        Ok(Self { token, guild_id, db_path, web_bind, web_url, relay })
    }
}
