//! Every message on the server, in this order; most are roleplay, and only cost a lookup.
//!
//! ```text
//!   message ─┬─ a bot's, a webhook's, another server's ─► ignored
//!            ├─ .prune · .set<slot> · .cookie         ─► dot::handle
//!            ├─ "nova: text"                          ─► proxy::post (counts the post as Nova)
//!            ├─ an Easter egg                         ─► its answer
//!            └─ in a roleplay channel                 ─► one post for the character the author plays
//! ```

use std::sync::Arc;

use anyhow::Result;
use twilight_model::channel::Message;
use twilight_model::channel::message::AllowedMentions;

use super::app::App;
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
    if let Some(reply) = eggs::answer(&message.content) {
        let mentions = AllowedMentions::default();
        app.http.create_message(message.channel_id).content(&reply).allowed_mentions(Some(&mentions)).await?;
    }
    if places::roleplay(&app, message.channel_id).await.unwrap_or(false)
        && let Some(active) = app.services.characters().active(message.author.id.get())?
    {
        app.services.proxy().count(active.id)?;
    }
    Ok(())
}
