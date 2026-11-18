//! Discord: the gateway, slash commands, forms and replies. Nothing here holds a rule; it reads what
//! people typed, asks the services, and says what happened.
//!
//! ```text
//!   gateway ─► run() ─┬─ GuildCreate / Role*  ─► App::observe       role names, for the staff check
//!                     ├─ Ready, Resumed, Gateway* ─► logged          connection health
//!                     └─ InteractionCreate    ─► interactions::handle (one task each, watched for panics)
//!                                                   ├─ weblinks   /z_moderator_admin, /me: web links, by DM
//!                                                   ├─ settings   /z_moderator_dbrs
//!                                                   ├─ spirit     /z_moderator_sp
//!                                                   ├─ characters /z_moderator_character
//!                                                   ├─ profile    /profile: a card, in the channel
//!                                                   └─ feats      /z_moderator_feat, Log feat
//!
//!   web's asks ─► guild::answer ─► names, channels, picture uploads and links, mod log lines
//! ```

mod app;
mod characters;
mod commands;
mod ctx;
mod feats;
mod guild;
mod interactions;
mod modlog;
mod profile;
mod settings;
mod spirit;
mod text;
mod weblinks;

use std::future::{Future, IntoFuture as _};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::{Instrument as _, error, info, warn};
use twilight_gateway::{Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt as _};
use twilight_http::Client;
use twilight_http::error::ErrorType;
use twilight_model::application::interaction::Interaction;

use crate::config::Config;
use crate::services::Services;
use crate::services::guild::Ask;
use crate::telemetry::{self, Counter};
use app::{App, Places};

// Guilds alone brings the server's roles and every interaction; nothing privileged yet.
const INTENTS: Intents = Intents::GUILDS;

// Only these are decoded; every other event is dropped before it costs memory. Closes always arrive.
const EVENTS: EventTypeFlags = EventTypeFlags::READY
    .union(EventTypeFlags::RESUMED)
    .union(EventTypeFlags::GATEWAY_RECONNECT)
    .union(EventTypeFlags::GATEWAY_INVALIDATE_SESSION)
    .union(EventTypeFlags::GUILD_CREATE)
    .union(EventTypeFlags::ROLE_CREATE)
    .union(EventTypeFlags::ROLE_UPDATE)
    .union(EventTypeFlags::ROLE_DELETE)
    .union(EventTypeFlags::INTERACTION_CREATE);

// Start-up calls retry with a growing wait: at boot the network or Discord may not answer yet.
const RETRY_FIRST: Duration = Duration::from_secs(5);
const RETRY_MAX: Duration = Duration::from_secs(5 * 60);
const HTTP_UNAUTHORIZED: u16 = 401;
const HTTP_FORBIDDEN: u16 = 403;

// A refused token never starts working: waiting on it would only hide the problem.
fn refused(source: &twilight_http::Error) -> bool {
    match source.kind() {
        ErrorType::Unauthorized => true,
        ErrorType::Response { status, .. } => matches!(status.get(), HTTP_UNAUTHORIZED | HTTP_FORBIDDEN),
        _ => false,
    }
}

async fn retried<T, F, Fut>(what: &str, mut call: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, twilight_http::Error>>,
{
    let mut wait = RETRY_FIRST;
    loop {
        match call().await {
            Ok(value) => return Ok(value),
            Err(source) if refused(&source) => return Err(source).with_context(|| format!("{what}: Discord refused the token")),
            Err(source) => {
                warn!(what, ?source, retry_in_s = wait.as_secs(), "start-up call failed; retrying");
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(RETRY_MAX);
            }
        }
    }
}

pub(crate) async fn run(config: Config, services: Arc<Services>, asks: UnboundedReceiver<Ask>) -> Result<()> {
    let http = Client::new(config.token.clone());
    let app = retried("reading the application", || http.current_user_application().into_future()).await?;
    let app_id = app.model().await.context("reading the application")?.id;

    let commands = commands::all();
    let interactions = http.interaction(app_id);
    retried("registering commands", || interactions.set_guild_commands(config.guild_id, &commands).into_future()).await?;
    info!(count = commands.len(), %app_id, "registered commands");

    // systemctl stop and restart send SIGTERM; a terminal sends Ctrl-C. Either stops cleanly.
    let mut terminate = signal(SignalKind::terminate()).context("listening for SIGTERM")?;

    let places = Places { web_url: config.web_url.clone(), pictures: config.pictures };
    let app = Arc::new(App::new(http, app_id, config.guild_id, services, places));
    tokio::spawn(telemetry::health(Arc::clone(&app.stats)));
    tokio::spawn(guild::answer(Arc::clone(&app), asks));
    let mut shard = Shard::new(ShardId::ONE, config.token, INTENTS);

    loop {
        let item = tokio::select! {
            item = shard.next_event(EVENTS) => item,
            _ = tokio::signal::ctrl_c() => {
                info!("stopping: interrupted");
                return Ok(());
            }
            _ = terminate.recv() => {
                info!("stopping: asked by systemd");
                return Ok(());
            }
        };
        // The shard gives up only on a close Discord marks fatal (a bad token, a disallowed intent).
        let Some(item) = item else {
            bail!("the gateway closed for good; the close above says why");
        };
        let event = match item {
            Ok(event) => event,
            Err(source) => {
                app.stats.bump(Counter::LostEvent);
                warn!(kind = ?source.kind(), ?source, "gateway event lost");
                continue;
            }
        };

        connection(&app, &event);
        app.observe(&event);
        if let Event::InteractionCreate(create) = event {
            spawn_interaction(&app, create.0);
        }
    }
}

// What the connection is doing, and anything that should not happen (a server it does not serve).
fn connection(app: &App, event: &Event) {
    match event {
        Event::Ready(ready) => info!(user = %ready.user.name, guilds = ready.guilds.len(), "connected"),
        Event::Resumed => info!("resumed"),
        Event::GatewayReconnect => {
            app.stats.bump(Counter::Reconnect);
            info!("Discord asked for a reconnect");
        }
        Event::GatewayInvalidateSession(resumable) => warn!(resumable, "session invalidated"),
        Event::GatewayClose(frame) => {
            let (code, reason) = frame.as_ref().map(|f| (f.code, f.reason.to_string())).unzip();
            warn!(?code, ?reason, "gateway closed");
        }
        Event::GuildCreate(guild) if guild.id() != app.guild_id => {
            warn!(guild = %guild.id(), "in a server it does not serve; its interactions are ignored");
        }
        _ => {}
    }
}

// One task per interaction, inside its span. A second task waits on it: a panic is counted and the
// caller is told, instead of the interaction hanging on "thinking…".
fn spawn_interaction(app: &Arc<App>, inter: Interaction) {
    let span = interactions::span_of(&inter);
    let (id, token) = (inter.id.get(), inter.token.clone());

    let worker = Arc::clone(app);
    let work = tokio::spawn(async move { interactions::handle(&worker, inter).await }.instrument(span.clone()));

    let watcher = Arc::clone(app);
    tokio::spawn(async move {
        let Err(join) = work.await else {
            return;
        };
        if !join.is_panic() {
            return;
        }
        watcher.stats.bump(Counter::Panicked);
        let reference = telemetry::reference_of(id);
        error!(parent: &span, %reference, "interaction panicked; the panic line above has the details");
        let said = interactions::failed_text(&reference);
        let update = watcher.http.interaction(watcher.app_id).update_response(&token).content(Some(&said)).await;
        if let Err(source) = update {
            warn!(parent: &span, ?source, "could not tell the caller about the panic");
        }
    });
}
