//! One interaction: who sent it, what they typed, and the ways to answer.

use anyhow::{Context, Result};
use tracing::{debug, info};
use twilight_model::application::command::{CommandOptionChoice, CommandOptionChoiceValue};
use twilight_model::application::interaction::application_command::{CommandData, CommandDataOption, CommandOptionValue};
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::Component;
use twilight_model::channel::message::{AllowedMentions, Embed, MessageFlags};
use twilight_model::guild::Permissions;
use twilight_model::http::interaction::{InteractionResponse, InteractionResponseData, InteractionResponseType};
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, RoleMarker, UserMarker};
use twilight_model::util::ImageHash;

use super::app::App;
use super::text::cut;
use crate::services::authority::{Caller, can_moderate};

pub(crate) const STAFF_ONLY: &str = "Staff only.";
// How much of a reply the debug log keeps.
const REPLY_LOGGED: usize = 300;

pub(crate) struct Ctx<'a> {
    pub(crate) app: &'a App,
    inter: &'a Interaction,
}

impl<'a> Ctx<'a> {
    pub(crate) const fn new(app: &'a App, inter: &'a Interaction) -> Self {
        Self { app, inter }
    }

    pub(crate) fn user_id(&self) -> Option<Id<UserMarker>> {
        let member_user = self.inter.member.as_ref().and_then(|m| m.user.as_ref());
        member_user.or(self.inter.user.as_ref()).map(|user| user.id)
    }

    /// The caller and their icon hashes: (id, server picture, own picture).
    pub(crate) fn caller_icons(&self) -> Option<(Id<UserMarker>, Option<ImageHash>, Option<ImageHash>)> {
        let member = self.inter.member.as_ref();
        let user = member.and_then(|m| m.user.as_ref()).or(self.inter.user.as_ref())?;
        Some((user.id, member.and_then(|m| m.avatar), user.avatar))
    }

    pub(crate) fn channel_id(&self) -> Option<Id<ChannelMarker>> {
        self.inter.channel.as_ref().map(|channel| channel.id)
    }

    pub(crate) fn is_staff(&self) -> bool {
        let Some(member) = self.inter.member.as_ref() else {
            return false;
        };
        let powers = member.permissions.unwrap_or_else(Permissions::empty);
        let role_names = self.app.role_names(&member.roles);
        can_moderate(&Caller {
            administrator: powers.contains(Permissions::ADMINISTRATOR),
            manages_server: powers.contains(Permissions::MANAGE_GUILD),
            role_names: &role_names,
        })
    }

    async fn respond(&self, kind: InteractionResponseType, data: InteractionResponseData) -> Result<()> {
        self.respond_with(kind, Some(data)).await
    }

    async fn respond_with(&self, kind: InteractionResponseType, data: Option<InteractionResponseData>) -> Result<()> {
        let response = InteractionResponse { kind, data };
        self.app
            .http
            .interaction(self.app.app_id)
            .create_response(self.inter.id, &self.inter.token, &response)
            .await
            .with_context(|| format!("answering the interaction ({kind:?})"))?;
        Ok(())
    }

    /// "Staff only.", and a line saying who tried: a player poking at staff commands shows up here.
    pub(crate) async fn deny(&self) -> Result<()> {
        let roles = self.inter.member.as_ref().map(|m| self.app.role_names(&m.roles)).unwrap_or_default();
        info!(?roles, "refused: not staff");
        self.whisper(STAFF_ONLY).await
    }

    /// An answer only the caller sees, with no pings.
    pub(crate) async fn whisper(&self, text: &str) -> Result<()> {
        debug!(reply = %cut(text, REPLY_LOGGED), "whisper");
        let data = InteractionResponseData {
            content: Some(text.to_owned()),
            flags: Some(MessageFlags::EPHEMERAL),
            allowed_mentions: Some(AllowedMentions::default()),
            ..InteractionResponseData::default()
        };
        self.respond(InteractionResponseType::ChannelMessageWithSource, data).await
    }

    /// Cards only the caller sees, with no pings: a guide.
    pub(crate) async fn whisper_embeds(&self, embeds: Vec<Embed>) -> Result<()> {
        debug!(count = embeds.len(), "whispered cards");
        let data = InteractionResponseData {
            embeds: Some(embeds),
            flags: Some(MessageFlags::EPHEMERAL),
            allowed_mentions: Some(AllowedMentions::default()),
            ..InteractionResponseData::default()
        };
        self.respond(InteractionResponseType::ChannelMessageWithSource, data).await
    }

    /// "noxaDBRS is thinking…", only to the caller: buys time for HTTP calls before `edit`.
    pub(crate) async fn defer(&self) -> Result<()> {
        let data = InteractionResponseData { flags: Some(MessageFlags::EPHEMERAL), ..InteractionResponseData::default() };
        self.respond(InteractionResponseType::DeferredChannelMessageWithSource, data).await
    }

    pub(crate) async fn edit(&self, text: &str) -> Result<()> {
        debug!(reply = %cut(text, REPLY_LOGGED), "reply");
        let mentions = AllowedMentions::default();
        self.app
            .http
            .interaction(self.app.app_id)
            .update_response(&self.inter.token)
            .content(Some(text))
            .allowed_mentions(Some(&mentions))
            .await
            .context("editing the deferred reply")?;
        Ok(())
    }

    /// A card everyone in the channel sees, with no pings.
    pub(crate) async fn show(&self, embed: Embed) -> Result<()> {
        debug!(title = ?embed.title, "card");
        let data = InteractionResponseData {
            embeds: Some(vec![embed]),
            allowed_mentions: Some(AllowedMentions::default()),
            ..InteractionResponseData::default()
        };
        self.respond(InteractionResponseType::ChannelMessageWithSource, data).await
    }

    /// A panel only the caller sees: an embed with controls under it.
    pub(crate) async fn panel(&self, embed: Embed, components: Vec<Component>) -> Result<()> {
        let data = InteractionResponseData {
            embeds: Some(vec![embed]),
            components: Some(components),
            flags: Some(MessageFlags::EPHEMERAL),
            allowed_mentions: Some(AllowedMentions::default()),
            ..InteractionResponseData::default()
        };
        self.respond(InteractionResponseType::ChannelMessageWithSource, data).await
    }

    /// The panel a control came from, redrawn in place.
    pub(crate) async fn update(&self, embed: Embed, components: Vec<Component>) -> Result<()> {
        let data = InteractionResponseData {
            embeds: Some(vec![embed]),
            components: Some(components),
            allowed_mentions: Some(AllowedMentions::default()),
            ..InteractionResponseData::default()
        };
        self.respond(InteractionResponseType::UpdateMessage, data).await
    }

    /// A second message only the caller sees, after the first answer.
    pub(crate) async fn followup(&self, text: &str) -> Result<()> {
        debug!(reply = %cut(text, REPLY_LOGGED), "followup");
        let mentions = AllowedMentions::default();
        self.app
            .http
            .interaction(self.app.app_id)
            .create_followup(&self.inter.token)
            .content(text)
            .flags(MessageFlags::EPHEMERAL)
            .allowed_mentions(Some(&mentions))
            .await
            .context("following up")?;
        Ok(())
    }

    /// The message a control or form was pressed on: (channel, message).
    pub(crate) fn source_message(&self) -> Option<(Id<ChannelMarker>, Id<twilight_model::id::marker::MessageMarker>)> {
        self.inter.message.as_ref().map(|m| (m.channel_id, m.id))
    }

    pub(crate) async fn modal(&self, custom_id: String, title: String, components: Vec<Component>) -> Result<()> {
        let data = InteractionResponseData {
            custom_id: Some(custom_id),
            title: Some(title),
            components: Some(components),
            ..InteractionResponseData::default()
        };
        self.respond(InteractionResponseType::Modal, data).await
    }

    /// An autocomplete menu: (shown, value) pairs.
    pub(crate) async fn choices(&self, choices: Vec<(String, String)>) -> Result<()> {
        let choices = choices
            .into_iter()
            .map(|(name, value)| CommandOptionChoice { name, name_localizations: None, value: CommandOptionChoiceValue::String(value) })
            .collect::<Vec<_>>();
        let data = InteractionResponseData { choices: Some(choices), ..InteractionResponseData::default() };
        self.respond(InteractionResponseType::ApplicationCommandAutocompleteResult, data).await
    }
}

/// The option every staff command picks its verb with: `/z_moderator_sp action:give …`.
pub(crate) const ACTION: &str = "action";

/// A command's options. `sub` is the verb: its `action` choice, or a subcommand's name.
pub(crate) struct Options<'a> {
    sub: Option<&'a str>,
    list: &'a [CommandDataOption],
}

impl<'a> Options<'a> {
    pub(crate) fn of(data: &'a CommandData) -> Self {
        if let [CommandDataOption { name, value: CommandOptionValue::SubCommand(inner) }] = data.options.as_slice() {
            return Self { sub: Some(name), list: inner };
        }
        let mut options = Self { sub: None, list: &data.options };
        options.sub = options.string(ACTION);
        options
    }

    pub(crate) const fn sub(&self) -> Option<&'a str> {
        self.sub
    }

    fn value(&self, name: &str) -> Option<&'a CommandOptionValue> {
        self.list.iter().find(|option| option.name == name).map(|option| &option.value)
    }

    pub(crate) fn string(&self, name: &str) -> Option<&'a str> {
        match self.value(name)? {
            CommandOptionValue::String(text) | CommandOptionValue::Focused(text, _) => Some(text),
            _ => None,
        }
    }

    pub(crate) fn integer(&self, name: &str) -> Option<i64> {
        match self.value(name)? {
            CommandOptionValue::Integer(n) => Some(*n),
            _ => None,
        }
    }

    pub(crate) fn user(&self, name: &str) -> Option<Id<UserMarker>> {
        match self.value(name)? {
            CommandOptionValue::User(id) => Some(*id),
            _ => None,
        }
    }

    pub(crate) fn role(&self, name: &str) -> Option<Id<RoleMarker>> {
        match self.value(name)? {
            CommandOptionValue::Role(id) => Some(*id),
            _ => None,
        }
    }

    pub(crate) fn channel(&self, name: &str) -> Option<Id<ChannelMarker>> {
        match self.value(name)? {
            CommandOptionValue::Channel(id) => Some(*id),
            _ => None,
        }
    }

    /// The option being typed in, for autocomplete: (name, text so far).
    pub(crate) fn focused(&self) -> Option<(&'a str, &'a str)> {
        self.list.iter().find_map(|option| match &option.value {
            CommandOptionValue::Focused(text, _) => Some((option.name.as_str(), text.as_str())),
            _ => None,
        })
    }
}
