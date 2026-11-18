//! Every message on the server, in this order; most are roleplay, and only cost a lookup.
//!
//! ```text
//!   message ─┬─ a bot's, a webhook's, another server's ─► ignored
//!            ├─ .prune · .set<slot> · .cookie         ─► dot::handle
//!            ├─ "nova: text"                          ─► proxy::post (counts the post as Nova)
//!            ├─ an Easter egg                         ─► its answer; "kakarot" by Z Broly's webhook
//!            └─ in a roleplay channel                 ─► one post for the character the author plays
//! ```

use std::sync::Arc;

use anyhow::Result;
use tracing::warn;
use twilight_model::channel::Message;
use twilight_model::channel::message::AllowedMentions;
use twilight_model::http::attachment::Attachment;

use super::app::App;
use super::eggs::{BROLY, BROLY_FILE, BROLY_JPEG, Egg, Voice};
use super::{dot, eggs, places, proxy};

pub(crate) async fn handle(app: Arc<App>, message: Message) -> Result<()> {
    if message.author.bot || message.webhook_id.is_some() || message.guild_id != Some(app.guild_id) {
        return Ok(());
    }
    if dot::handle(&app, &message).await? {
        return Ok(());
    }
    if proxy::post(&app, &message).await? {
        return Ok(());
    }
    if let Some(egg) = eggs::answer(&message.content) {
        hatch(&app, &message, &egg).await?;
    }
    if places::roleplay(&app, message.channel_id).await.unwrap_or(false)
        && let Some(active) = app.services.characters().active(message.author.id.get())?
    {
        app.services.proxy().count(active.id)?;
    }
    Ok(())
}

// An egg's answer: Broly's through his own webhook, with his face; the bot's own if that fails.
async fn hatch(app: &App, message: &Message, egg: &Egg) -> Result<()> {
    if egg.voice == Voice::Broly {
        match as_broly(app, message, &egg.text).await {
            Ok(()) => return Ok(()),
            Err(source) => warn!(?source, "Z Broly could not answer; the bot does"),
        }
    }
    let mentions = AllowedMentions::default();
    app.http.create_message(message.channel_id).content(&egg.text).allowed_mentions(Some(&mentions)).await?;
    Ok(())
}

async fn as_broly(app: &App, message: &Message, text: &str) -> Result<()> {
    let spot = places::spot(app, message.channel_id).await?;
    let hook = places::webhook(app, spot.hook_channel, &BROLY).await?;
    let mentions = AllowedMentions::default();
    let face = [Attachment::from_bytes(BROLY_FILE.to_owned(), BROLY_JPEG.to_vec(), 1)];
    let mut request = app.http.execute_webhook(hook.id, &hook.token).content(text).attachments(&face).allowed_mentions(Some(&mentions));
    if let Some(thread) = spot.thread {
        request = request.thread_id(thread);
    }
    request.await?;
    Ok(())
}
