//! Posting as a character, Bot1's way: `nova: text` is posted again under the character's name and
//! picture through a webhook, and the original goes.
//!
//! ```text
//!   "nova:happy Hi" ─► whose prefix? the author's own character, else (staff) one of the cast
//!                   ─► the webhook of the channel (a thread's parent), as Nova, with happy's picture
//!                   ─► parts of 1800, each after the first linked back; a reply line on the last
//!                   ─► attachments again on the first part ─► the original deleted ─► one post counted
//!   ❌ on a post (its author only) ─► every part deleted
//!   ✏️ on a post (its author only) ─► [Edit ✏️] ─► a form with its text ─► the part edited
//! ```
//!
//! Needs Manage Webhooks and Manage Messages where it posts.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use tracing::{info, warn};
use twilight_model::application::interaction::message_component::MessageComponentInteractionData;
use twilight_model::application::interaction::modal::ModalInteractionData;
use twilight_model::channel::Message;
use twilight_model::channel::message::component::{ButtonStyle, TextInputStyle};
use twilight_model::channel::message::{AllowedMentions, EmojiReactionType, MentionType};
use twilight_model::gateway::GatewayReaction;
use twilight_model::http::attachment::Attachment;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, MessageMarker};
use twilight_http::request::channel::reaction::RequestReactionType;
use twilight_util::builder::embed::EmbedBuilder;

use super::app::App;
use super::ctx::Ctx;
use super::guild::picture_of;
use super::places;
use super::profile::icon_url;
use super::widgets::{TextSpec, button, field, labelled, row, text_input};
use crate::domain::proxy::{PART_MAX, continued_line, parse, parts, reply_line, split_headers};
use crate::domain::tutorial::Did;
use crate::services::proxy::{Cast, Found};
use crate::store::proxied::ProxiedRow;

const PREFIX: &str = "noxadbrs|px|";
const EDIT: &str = "edit";
const SAVE: &str = "save";
const TEXT_FIELD: &str = "text";
const USERNAME_MAX: usize = 80;
const MESSAGE_MAX: usize = 2000;
const EMBED_TEXT_MAX: usize = 4000;
const PROMPT_FOR: Duration = Duration::from_secs(5 * 60);
// What a post may carry again: Discord takes 10 MiB from a bot in all.
const FILES_MAX: usize = 10 * 1024 * 1024;
const DELETE: [&str; 6] = ["❌", "❎", "🔴", "🟥", "🔺", "🔻"];
const EDIT_MARKS: [&str; 7] = ["✏️", "📝", "🖊️", "🖋️", "🟡", "🟨", "💛"];
const GONE: &str = "That post is gone.";

/// What a reaction asks of a post.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    Delete,
    Edit,
}

// Bot1's marks: the red crosses and squares, the pens and yellows; a custom emoji by its name.
fn ask_of(emoji: &EmojiReactionType) -> Option<Ask> {
    let unicode = |name: &str| {
        let bare = name.trim_end_matches('\u{fe0f}');
        if DELETE.iter().any(|d| d.trim_end_matches('\u{fe0f}') == bare) {
            return Some(Ask::Delete);
        }
        EDIT_MARKS.iter().any(|e| e.trim_end_matches('\u{fe0f}') == bare).then_some(Ask::Edit)
    };
    match emoji {
        EmojiReactionType::Unicode { name } => unicode(name),
        EmojiReactionType::Custom { name: Some(name), .. } => {
            let name = name.to_lowercase();
            if name.contains("cross_mark") || name.split(['_', '-']).any(|w| w == "red") {
                return Some(Ask::Delete);
            }
            ["pen", "pencil", "yellow"].iter().any(|w| name.contains(w)).then_some(Ask::Edit)
        }
        EmojiReactionType::Custom { name: None, .. } => None,
    }
}

fn message_url(app: &App, channel: Id<ChannelMarker>, message: Id<MessageMarker>) -> String {
    format!("https://discord.com/channels/{}/{channel}/{message}", app.guild_id)
}

fn users_only() -> AllowedMentions {
    AllowedMentions { parse: vec![MentionType::Users], ..AllowedMentions::default() }
}

// The line that says who a post answers: a post made as a character names it and pings its author.
fn reply_of(app: &App, message: &Message) -> Result<Option<String>> {
    let Some(answered) = message.referenced_message.as_deref() else {
        return Ok(None);
    };
    let url = message_url(app, answered.channel_id, answered.id);
    let (name, user) = match app.services.proxy().post(answered.id.get())? {
        Some(record) => (answered.author.name.clone(), record.author_id),
        None => (answered.author.global_name.clone().unwrap_or_else(|| answered.author.name.clone()), answered.author.id.get()),
    };
    // A reply with its ping turned off pings nobody here either.
    let pinged = message.mentions.iter().any(|m| m.id.get() == user);
    if user == message.author.id.get() || !pinged {
        return Ok(Some(format!("> [Reply to]({url}): **{}**", super::text::plain(&name))));
    }
    Ok(Some(reply_line(&url, &super::text::plain(&name), user)))
}

// The attachments of the original, fetched to post again; one that fails is left out and logged.
async fn files_of(message: &Message) -> Vec<Attachment> {
    let mut files = Vec::new();
    let mut total = 0;
    for (n, file) in message.attachments.iter().enumerate() {
        let size = usize::try_from(file.size).unwrap_or(usize::MAX);
        if total + size > FILES_MAX {
            warn!(file = %file.filename, "an attachment over what a post may carry was left out");
            continue;
        }
        match crate::net::fetch(&file.url, FILES_MAX).await {
            Ok(bytes) => {
                total += bytes.len();
                files.push(Attachment::from_bytes(file.filename.clone(), bytes, u64::try_from(n).unwrap_or_default() + 1));
            }
            Err(source) => warn!(?source, file = %file.filename, "an attachment could not be fetched again"),
        }
    }
    files
}

// The picture a post goes out with: the expression's, the character's, else (a player's) their icon.
async fn avatar_of(app: &App, message: &Message, found: &Found) -> Option<String> {
    if let Some(expression) = &found.expression {
        return Some(expression.url.clone());
    }
    if let Some(picture) = picture_of(app, &found.character).await {
        return Some(picture);
    }
    if found.npc {
        return None;
    }
    let server = message.member.as_ref().and_then(|m| m.avatar);
    Some(icon_url(app.guild_id, message.author.id, server, message.author.avatar))
}

/// Posts a message again as a character, if it starts with one's prefix. True when it did.
pub(crate) async fn post(app: &Arc<App>, message: &Message) -> Result<bool> {
    let Some(asked) = parse(&message.content) else {
        return Ok(false);
    };
    let author = message.author.id;
    let roles = message.member.as_ref().map(|m| m.roles.as_slice()).unwrap_or_default();
    let cast = if app.is_staff_member(author, roles) { Cast::Allowed } else { Cast::Denied };
    let Some(found) = app.services.proxy().find(author.get(), &asked.prefix, asked.expression.as_deref(), cast)? else {
        return Ok(false);
    };

    let spot = places::spot(app, message.channel_id).await?;
    let hook = match places::hook(app, spot.hook_channel).await {
        Ok(hook) => hook,
        Err(source) => {
            warn!(?source, channel = %spot.hook_channel, "no webhook to post as a character; the message stays");
            return Ok(false);
        }
    };
    let name: String = found.character.name.chars().take(USERNAME_MAX).collect();
    let avatar = avatar_of(app, message, &found).await;
    let color = found.expression.as_ref().and_then(|e| e.color);
    let reply = reply_of(app, message)?;
    let files = files_of(message).await;
    let pieces = parts(asked.body, PART_MAX);
    let mentions = users_only();

    let mut sent: Vec<ProxiedRow> = Vec::new();
    for (i, piece) in pieces.iter().enumerate() {
        let mut lines = Vec::new();
        if let Some(previous) = sent.last() {
            lines.push(continued_line(i + 1, pieces.len(), &message_url(app, message.channel_id, Id::new(previous.message_id))));
        }
        if color.is_none() {
            lines.push(piece.clone());
        }
        let last = i + 1 == pieces.len();
        if let (true, Some(reply)) = (last, reply.as_deref()) {
            lines.push(reply.to_owned());
        }
        let content = lines.join("\n");
        let embeds: Vec<_> = color.map(|c| EmbedBuilder::new().description(piece.clone()).color(c).build()).into_iter().collect();

        let mut request = app.http.execute_webhook(hook.id, &hook.token).username(&name).allowed_mentions(Some(&mentions)).embeds(&embeds);
        if !content.is_empty() {
            request = request.content(&content);
        }
        if let Some(avatar) = avatar.as_deref() {
            request = request.avatar_url(avatar);
        }
        if let Some(thread) = spot.thread {
            request = request.thread_id(thread);
        }
        if i == 0 && !files.is_empty() {
            request = request.attachments(&files);
        }
        let posted = match request.wait().await {
            Ok(response) => response.model().await?,
            Err(source) => {
                // A webhook someone deleted: forgotten, so the next post makes a new one.
                places::forget_hook(app, spot.hook_channel);
                warn!(?source, "posting as a character failed; the message stays");
                return Ok(!sent.is_empty());
            }
        };
        let group_id = sent.first().map_or(posted.id.get(), |first| first.message_id);
        sent.push(ProxiedRow {
            message_id: posted.id.get(),
            channel_id: message.channel_id.get(),
            hook_channel: spot.hook_channel.get(),
            thread_id: spot.thread.map(Id::get),
            webhook_id: hook.id.get(),
            author_id: author.get(),
            character_id: found.character.id,
            group_id,
            part: i64::try_from(i + 1).unwrap_or(i64::MAX),
            at: 0,
        });
    }
    app.services.proxy().record(&sent)?;
    if let Err(source) = app.http.delete_message(message.channel_id, message.id).await {
        warn!(?source, "the original of a post as a character could not be deleted: give the bot Manage Messages");
    }
    info!(character = %found.character.name, npc = found.npc, parts = sent.len(), "posted as a character");

    if places::roleplay(app, message.channel_id).await.unwrap_or(false) {
        app.services.proxy().count(found.character.id)?;
    }
    super::tutorial::observe(app, author.get(), Did::Proxied { expression: found.expression.is_some(), npc: found.npc }).await;
    Ok(true)
}

/// A reaction on a post made as a character: ❌ deletes it, ✏️ offers to edit it; its author's only.
pub(crate) async fn reacted(app: &Arc<App>, reaction: &GatewayReaction) -> Result<()> {
    let Some(ask) = ask_of(&reaction.emoji) else {
        return Ok(());
    };
    let Some(record) = app.services.proxy().post(reaction.message_id.get())? else {
        return Ok(());
    };
    if record.author_id != reaction.user_id.get() {
        return Ok(());
    }
    match ask {
        Ask::Delete => {
            for part in app.services.proxy().group(record.group_id)? {
                if let (Some(channel), Some(message)) = (Id::new_checked(part.channel_id), Id::new_checked(part.message_id)) {
                    app.http.delete_message(channel, message).await.ok();
                }
                app.services.proxy().forget(part.message_id)?;
            }
            info!("a post as a character deleted by its author");
            super::tutorial::observe(app, reaction.user_id.get(), Did::ProxyDeleted).await;
        }
        Ask::Edit => {
            let emoji = match &reaction.emoji {
                EmojiReactionType::Unicode { name } => RequestReactionType::Unicode { name },
                EmojiReactionType::Custom { id, name, .. } => RequestReactionType::Custom { id: *id, name: name.as_deref() },
            };
            app.http.delete_reaction(reaction.channel_id, reaction.message_id, &emoji, reaction.user_id).await.ok();
            let buttons = [row(vec![button(format!("{PREFIX}{EDIT}|{}", record.message_id), "Edit ✏️", ButtonStyle::Primary)])];
            let mentions = users_only();
            let prompt = app
                .http
                .create_message(reaction.channel_id)
                .content(&format!("<@{}> ✏️", reaction.user_id))
                .components(&buttons)
                .allowed_mentions(Some(&mentions))
                .await?
                .model()
                .await?;
            let (app, channel) = (Arc::clone(app), reaction.channel_id);
            tokio::spawn(async move {
                tokio::time::sleep(PROMPT_FOR).await;
                app.http.delete_message(channel, prompt.id).await.ok();
            });
        }
    }
    Ok(())
}

pub(crate) fn owns(custom_id: &str) -> bool {
    custom_id.starts_with(PREFIX)
}

fn target_of(custom_id: &str, verb: &str) -> Option<u64> {
    custom_id.strip_prefix(PREFIX)?.strip_prefix(verb)?.strip_prefix('|')?.parse().ok()
}

// The post's text, and whether it sits in an embed (an expression with a colour bar).
async fn text_of(app: &App, record: &ProxiedRow) -> Result<Option<(String, bool)>> {
    let (Some(channel), Some(message)) = (Id::new_checked(record.channel_id), Id::new_checked(record.message_id)) else {
        return Ok(None);
    };
    let Ok(response) = app.http.message(channel, message).await else {
        return Ok(None);
    };
    let posted = response.model().await?;
    if let Some(text) = posted.embeds.first().and_then(|e| e.description.clone()) {
        return Ok(Some((text, true)));
    }
    Ok(Some((split_headers(&posted.content).1.to_owned(), false)))
}

/// The Edit button: a form with the part's text, for its author only.
pub(crate) async fn component(ctx: &Ctx<'_>, data: &MessageComponentInteractionData) -> Result<()> {
    let Some(target) = target_of(&data.custom_id, EDIT) else {
        return Ok(());
    };
    let Some(record) = ctx.app.services.proxy().post(target)? else {
        return ctx.whisper(GONE).await;
    };
    if ctx.user_id().map(Id::get) != Some(record.author_id) {
        return ctx.whisper("Only its author can edit it.").await;
    }
    let Some((text, embedded)) = text_of(ctx.app, &record).await? else {
        return ctx.whisper(GONE).await;
    };
    let max = if embedded { EMBED_TEXT_MAX } else { MESSAGE_MAX - 200 };
    let input = text_input(TextSpec { custom_id: TEXT_FIELD, style: TextInputStyle::Paragraph, max: u16::try_from(max).unwrap_or(u16::MAX), value: Some(text), placeholder: None, required: true });
    ctx.modal(format!("{PREFIX}{SAVE}|{target}"), "Edit post".to_owned(), vec![labelled("Text", None, input)]).await
}

/// The edit form sent: the part changes, its header lines kept.
pub(crate) async fn submit(ctx: &Ctx<'_>, data: &ModalInteractionData) -> Result<()> {
    let Some(target) = target_of(&data.custom_id, SAVE) else {
        return Ok(());
    };
    let Some(record) = ctx.app.services.proxy().post(target)? else {
        return ctx.whisper(GONE).await;
    };
    if ctx.user_id().map(Id::get) != Some(record.author_id) {
        return ctx.whisper("Only its author can edit it.").await;
    }
    let text = field(&data.components, TEXT_FIELD).unwrap_or_default();
    let (Some(channel), Some(message), Some(hook_channel)) = (Id::new_checked(record.channel_id), Id::new_checked(record.message_id), Id::new_checked(record.hook_channel)) else {
        return ctx.whisper(GONE).await;
    };
    let posted = match ctx.app.http.message(channel, message).await {
        Ok(response) => response.model().await?,
        Err(_) => return ctx.whisper(GONE).await,
    };
    let hook = places::hook(ctx.app, hook_channel).await?;
    if hook.id.get() != record.webhook_id {
        return ctx.whisper("That post's webhook is gone, so it cannot be edited. Delete it with ❌ and post again.").await;
    }

    let mut request = ctx.app.http.update_webhook_message(hook.id, &hook.token, message);
    if let Some(thread) = record.thread_id.and_then(Id::new_checked) {
        request = request.thread_id(thread);
    }
    let updated = if let Some(embed) = posted.embeds.first() {
        let mut edited = embed.clone();
        edited.description = Some(text);
        let embeds = [edited];
        request.embeds(Some(&embeds)).await
    } else {
        let (continued, _, reply) = split_headers(&posted.content);
        let content = [continued, Some(text.as_str()), reply].into_iter().flatten().collect::<Vec<_>>().join("\n");
        if content.chars().count() > MESSAGE_MAX {
            return ctx.whisper(&format!("That is too long for one part: {MESSAGE_MAX} characters at most, the header lines with it.")).await;
        }
        request.content(Some(&content)).await
    };
    updated.context("editing a post as a character")?;
    if let Some((prompt_channel, prompt)) = ctx.source_message() {
        ctx.app.http.delete_message(prompt_channel, prompt).await.ok();
    }
    ctx.whisper("Edited.").await?;
    super::tutorial::observe(ctx.app, record.author_id, Did::ProxyEdited).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unicode(name: &str) -> EmojiReactionType {
        EmojiReactionType::Unicode { name: name.to_owned() }
    }

    fn custom(name: &str) -> EmojiReactionType {
        EmojiReactionType::Custom { animated: false, id: Id::new(1), name: Some(name.to_owned()) }
    }

    #[test]
    fn crosses_delete_pens_edit_anything_else_is_a_reaction() {
        assert_eq!(ask_of(&unicode("❌")), Some(Ask::Delete));
        assert_eq!(ask_of(&unicode("🟥")), Some(Ask::Delete));
        assert_eq!(ask_of(&unicode("✏️")), Some(Ask::Edit));
        assert_eq!(ask_of(&unicode("✏")), Some(Ask::Edit));
        assert_eq!(ask_of(&unicode("💛")), Some(Ask::Edit));
        assert_eq!(ask_of(&unicode("😂")), None);
        assert_eq!(ask_of(&custom("cross_mark_2")), Some(Ask::Delete));
        assert_eq!(ask_of(&custom("big_red_x")), Some(Ask::Delete));
        assert_eq!(ask_of(&custom("reddit")), None);
        assert_eq!(ask_of(&custom("golden_pen")), Some(Ask::Edit));
    }

    #[test]
    fn buttons_and_forms_name_the_post() {
        assert_eq!(target_of("noxadbrs|px|edit|123", EDIT), Some(123));
        assert_eq!(target_of("noxadbrs|px|save|123", SAVE), Some(123));
        assert_eq!(target_of("noxadbrs|px|save|123", EDIT), None);
    }
}
