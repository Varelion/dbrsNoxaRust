//! Routes each interaction to its handler: slash and menu commands by name, autocomplete by command,
//! forms by their custom id. Each one's outcome is logged inside its span, with how long it took.

use std::time::Instant;

use anyhow::Result;
use tracing::{Span, debug, error, info, info_span, warn};
use twilight_model::application::interaction::{Interaction, InteractionData, InteractionType};

use super::app::App;
use super::ctx::{Ctx, Options};
use super::{characters, commands, feats, profile, settings, spirit, tutorial, weblinks};
use crate::telemetry::{Counter, reference_of};

/// What a caller reads when their command failed; the reference finds it in the journal.
pub(crate) fn failed_text(reference: &str) -> String {
    format!("That did not work (ref {reference}). Staff can find it in the bot's log.")
}

// The command's name with its action ("z_moderator_sp give"), or a form's kind.
fn name_of(inter: &Interaction) -> String {
    match inter.data.as_ref() {
        Some(InteractionData::ApplicationCommand(data)) => match Options::of(data).sub() {
            Some(sub) => format!("{} {sub}", data.name),
            None => data.name.clone(),
        },
        Some(InteractionData::ModalSubmit(data)) => data.custom_id.split('|').take(2).collect::<Vec<_>>().join("|"),
        Some(InteractionData::MessageComponent(data)) => data.custom_id.clone(),
        None => String::new(),
        Some(_) => "unknown".to_owned(),
    }
}

/// The span every line about this interaction sits in.
pub(crate) fn span_of(inter: &Interaction) -> Span {
    let user = inter.member.as_ref().and_then(|m| m.user.as_ref()).or(inter.user.as_ref()).map(|u| u.id.get());
    let channel = inter.channel.as_ref().map(|c| c.id.get());
    info_span!("interaction", id = inter.id.get(), kind = ?inter.kind, command = %name_of(inter), user, channel)
}

pub(crate) async fn handle(app: &App, inter: Interaction) -> Result<()> {
    // noxaDBRS serves one server; anything else (a DM, another server) is ignored.
    if inter.guild_id != Some(app.guild_id) {
        warn!(guild = ?inter.guild_id, "interaction from elsewhere ignored");
        return Ok(());
    }

    let started = Instant::now();
    let ctx = Ctx::new(app, &inter);
    let handled = route(&ctx, &inter).await;
    let ms = started.elapsed().as_millis();

    match &handled {
        // Autocomplete fires on every keystroke: kept out of the info level.
        Ok(()) if inter.kind == InteractionType::ApplicationCommandAutocomplete => debug!(ms, "done"),
        Ok(()) => {
            app.stats.bump(Counter::Done);
            info!(ms, "done");
        }
        Err(source) => {
            app.stats.bump(Counter::Failed);
            let reference = reference_of(inter.id.get());
            error!(%reference, ms, error = ?source, "failed");
            // Most handlers have deferred by the time they fail: tell the caller instead of leaving them waiting.
            ctx.edit(&failed_text(&reference)).await.ok();
        }
    }
    handled
}

async fn route(ctx: &Ctx<'_>, inter: &Interaction) -> Result<()> {
    match (inter.kind, inter.data.as_ref()) {
        (InteractionType::ApplicationCommandAutocomplete, Some(InteractionData::ApplicationCommand(data))) => {
            let opts = Options::of(data);
            match data.name.as_str() {
                commands::PROFILE => profile::complete(ctx, &opts).await,
                commands::FEAT => feats::complete(ctx, &opts).await,
                other => {
                    warn!(command = other, "autocomplete for a command without menus");
                    ctx.choices(Vec::new()).await
                }
            }
        }
        (InteractionType::ApplicationCommand, Some(InteractionData::ApplicationCommand(data))) => {
            let opts = Options::of(data);
            match data.name.as_str() {
                commands::ADMIN => weblinks::admin(ctx).await,
                commands::ME => weblinks::me(ctx).await,
                commands::TUTORIAL => tutorial::show(ctx, &opts).await,
                commands::DBRS => settings::admin(ctx, &opts).await,
                commands::SP => spirit::admin(ctx, &opts).await,
                commands::CHARACTER => characters::admin(ctx, &opts).await,
                commands::PROFILE => profile::show(ctx, &opts, data).await,
                commands::FEAT => feats::admin(ctx, &opts).await,
                commands::LOG_FEAT => feats::log_feat(ctx, data).await,
                // A command Discord still shows but this build no longer has: registration is stale.
                other => {
                    warn!(command = other, "unknown command; restart to re-register");
                    ctx.whisper("That command is gone. Staff: restart the bot to refresh the list.").await
                }
            }
        }
        // Old /panel buttons, still on messages from before /me.
        (InteractionType::MessageComponent, Some(InteractionData::MessageComponent(data))) => {
            warn!(id = %data.custom_id, "a control nobody handles");
            ctx.whisper("That panel is gone: use /me for your page.").await
        }
        (InteractionType::ModalSubmit, Some(InteractionData::ModalSubmit(data))) => feats::submit(ctx, data).await,
        (kind, _) => {
            warn!(?kind, "interaction with nothing to handle it");
            Ok(())
        }
    }
}
