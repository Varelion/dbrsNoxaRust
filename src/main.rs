//! noxaDBRS: Dragon Ball: Rising Spirit's bot.
//!
//! ```text
//!   main ─┬─► discord (gateway, commands, panels) ─┐
//!         ├─► web (admin pages for staff) ──────────┼─► services (rules) ─► store (SQLite) ─► noxadbrs.db
//!         │     ├─ guild asks ─► discord: names, channels, pictures, mod log lines
//!         │     └─ relay ─► wss://dbrs.neokai.org: the pages, public, no port opened
//!         │            └─ net: DNS over HTTPS, TLS 1.3 with Encrypted Client Hello
//!         └─► telemetry (logs, panics, health)                  domain (pure rules, no I/O)
//! ```

mod config;
mod discord;
mod domain;
mod net;
mod services;
mod store;
mod telemetry;
mod web;

use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Result;
use tracing::{error, info};

use config::Config;
use services::Services;
use services::guild::Guild;

// One thread: a bot for one server waits on Discord, not on the CPU, and each extra worker costs memory.
#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    telemetry::init();
    match start().await {
        Ok(()) => ExitCode::SUCCESS,
        // Logged in full (with its causes) before exiting non-zero, so systemd restarts it and the journal says why.
        Err(source) => {
            error!(error = ?source, "stopped on an error");
            ExitCode::FAILURE
        }
    }
}

async fn start() -> Result<()> {
    install_tls()?;
    let config = Config::from_env()?;
    info!(version = env!("CARGO_PKG_VERSION"), db = %config.db_path.display(), guild = %config.guild_id, "starting");

    let services = Arc::new(Services::open(&config.db_path)?);
    let (guild, asks) = Guild::new(config.guild_id.get());

    // The admin pages are optional: if they cannot start, the bot runs without them and says why.
    // Uploads go through the relay Worker, so only a bot with a relay signs upload tickets.
    let tickets = config.relay.as_ref().map(|relay| crate::services::tickets::Tickets::new(&relay.secret));
    let routes = web::routes(Arc::clone(&services), guild, tickets);
    if let Some(bind) = config.web_bind {
        let local = routes.clone();
        tokio::spawn(async move {
            if let Err(source) = web::serve(bind, local).await {
                error!(error = ?source, "the web pages stopped");
            }
        });
    }
    if let Some(relay) = config.relay.as_ref() {
        tokio::spawn(web::relay::run(relay.url.clone(), relay.secret.clone(), routes));
    }
    discord::run(config, services, asks).await
}

// rustls needs one crypto provider chosen up front; ring builds on a Raspberry Pi without extra tooling.
fn install_tls() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| anyhow::anyhow!("a TLS provider was already installed"))
}
