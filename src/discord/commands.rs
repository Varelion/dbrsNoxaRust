//! Every command noxaDBRS registers on the server, as Discord shows them. Handlers match on these names.
//! Staff commands read z_moderator_<thing>, so they sort last in Discord's list; each has one `action`
//! choice for its verb, and everything else is managed on the admin pages (/z_moderator_admin).
//!
//! ```text
//!   /z_moderator_admin                       a 30-minute admin link, by DM
//!   /z_moderator_dbrs      action:mod_log    settings
//!   /z_moderator_sp        action:give|take|set|grant|history
//!   /z_moderator_character action:add
//!   /z_moderator_feat      action:add|edit|link|unlink|archive|restore|list
//!   /mypage                                      a player's own page, a link by DM: characters, SP
//!   /profile                                 a character's card, in the channel
//!   /tutorial [guide:players|staff]          how to use the bot, only to the caller
//!   /help [topic]                            a guide, or any command's page
//!   .prune <count>                           a message, not a slash command (dot.rs)
//!   Log feat                                 right-click a post › Apps
//! ```

use twilight_model::application::command::{Command, CommandType};
use twilight_model::channel::ChannelType;
use twilight_util::builder::command::{ChannelBuilder, CommandBuilder, IntegerBuilder, RoleBuilder, StringBuilder, UserBuilder};

use super::characters::CharacterSub;
use super::ctx::ACTION;
use super::feats::{FeatSub, FormField};
use super::settings::SettingsSub;
use super::help::TOPIC;
use super::tutorial::{GUIDE, Guide};
use crate::domain::feats::{FeatCounts, FeatKind, FeatState, NOTE_MAX, REASON_MAX};
use crate::domain::spirit::{MEMO_MAX, SP_MAX, SpAction};
use crate::services::characters::{NAME_MAX, SLOT_MAX};

pub(crate) const ADMIN: &str = "z_moderator_admin";
pub(crate) const DBRS: &str = "z_moderator_dbrs";
pub(crate) const SP: &str = "z_moderator_sp";
pub(crate) const CHARACTER: &str = "z_moderator_character";
pub(crate) const PROFILE: &str = "profile";
pub(crate) const MYPAGE: &str = "mypage";
pub(crate) const TUTORIAL: &str = "tutorial";
pub(crate) const HELP: &str = "help";
pub(crate) const FEAT: &str = "z_moderator_feat";
pub(crate) const LOG_FEAT: &str = "Log feat";

pub(crate) const HISTORY: &str = "history";

pub(crate) fn all() -> Vec<Command> {
    vec![admin(), dbrs(), sp(), character(), mypage(), profile(), tutorial(), help(), feat(), log_feat()]
}

// The verb, picked from a list: (shown, value) pairs.
fn action<'a>(choices: impl IntoIterator<Item = (&'a str, &'a str)>) -> StringBuilder {
    StringBuilder::new(ACTION, "What to do").choices(choices).required(true)
}

fn admin() -> Command {
    CommandBuilder::new(ADMIN, "Staff: the admin pages, a link by DM, good for 30 minutes", CommandType::ChatInput).build()
}

fn dbrs() -> Command {
    let channel = ChannelBuilder::new("channel", "e.g. #mod-log").channel_types([ChannelType::GuildText]);
    CommandBuilder::new(DBRS, "Staff: Raging Spirit settings", CommandType::ChatInput)
        .option(action(SettingsSub::ALL.iter().map(|s| (s.label(), s.key()))))
        .option(channel)
        .build()
}

fn mypage() -> Command {
    CommandBuilder::new(MYPAGE, "Your page: characters, who you play, Spirit Points; a link by DM", CommandType::ChatInput).build()
}

fn sp() -> Command {
    let choices = [
        (SpAction::Give, "Give: add to one player's Spirit Points"),
        (SpAction::Take, "Take: remove from one player's Spirit Points"),
        (SpAction::Set, "Set: one player's Spirit Points become the amount"),
        (SpAction::Grant, "Grant: everyone holding a role gets the amount"),
    ];
    let verbs = choices.iter().map(|(a, label)| (*label, a.key())).chain([("History: a player's balance and last ledger lines", HISTORY)]);

    CommandBuilder::new(SP, "Staff: Spirit Points: give, take, set, grant, history", CommandType::ChatInput)
        .option(action(verbs))
        .option(UserBuilder::new("player", "Whose Spirit Points (give, take, set, history)"))
        .option(IntegerBuilder::new("amount", format!("How many, up to {SP_MAX}")).min_value(0).max_value(SP_MAX))
        .option(StringBuilder::new("memo", "Why: kept in the ledger and the mod log").max_length(MEMO_MAX as u16))
        .option(RoleBuilder::new("role", "Grant: e.g. @Accepted"))
        .build()
}

fn character() -> Command {
    CommandBuilder::new(CHARACTER, "Staff: a player's characters", CommandType::ChatInput)
        .option(action(CharacterSub::ALL.iter().map(|s| (s.label(), s.key()))))
        .option(UserBuilder::new("player", "Whose character"))
        .option(IntegerBuilder::new("slot", format!("1 to {SLOT_MAX}")).min_value(1).max_value(SLOT_MAX))
        .option(StringBuilder::new("name", "The character's name").max_length(NAME_MAX as u16))
        .build()
}

fn tutorial() -> Command {
    CommandBuilder::new(TUTORIAL, "How to use the bot, step by step, only to you", CommandType::ChatInput)
        .option(StringBuilder::new(GUIDE, "Which guide; players when left out").choices(Guide::ALL.iter().map(|g| (g.label(), g.key()))))
        .build()
}

fn help() -> Command {
    CommandBuilder::new(HELP, "A guide, or any command's page: what it does and how to type it", CommandType::ChatInput)
        .option(StringBuilder::new(TOPIC, "A guide or a command: start, cards, /profile …; the list when left out").autocomplete(true))
        .build()
}

fn profile() -> Command {
    CommandBuilder::new(PROFILE, "A character's card", CommandType::ChatInput)
        .option(UserBuilder::new("player", "Whose; yours when left out"))
        .option(StringBuilder::new("character", "Which one; the one they play when left out").autocomplete(true))
        .build()
}

fn feat() -> Command {
    let kinds = FeatKind::ALL.iter().map(|k| (k.label(), k.key()));
    let counts = FeatCounts::ALL.iter().map(|c| (format!("Counts as: {}", c.label()), c.key()));
    let states = [("Active", FeatState::Active.key()), ("Archived", FeatState::Archived.key())];

    CommandBuilder::new(FEAT, "Staff: silent feats, linked to posts, for tier-up reviews", CommandType::ChatInput)
        .option(action(FeatSub::ALL.iter().map(|s| (s.label(), s.key()))))
        .option(StringBuilder::new("feat", "Pick from the menu: F-0012 · Itami …").autocomplete(true))
        .option(UserBuilder::new("player", "Whose character (add, list)"))
        .option(StringBuilder::new("character", "One of their characters (add, list)").autocomplete(true))
        .option(StringBuilder::new(FormField::Kind.key(), "What happened").choices(kinds))
        .option(StringBuilder::new(FormField::Counts.key(), "What it stands for in a tier-up").choices(counts))
        .option(StringBuilder::new(FormField::Note.key(), "What staff saw").max_length(NOTE_MAX as u16))
        .option(StringBuilder::new("link", "A message link: right-click a post › Copy Message Link").autocomplete(true))
        .option(StringBuilder::new("reason", "Archive: why").max_length(REASON_MAX as u16))
        .option(StringBuilder::new("state", "List: active (default) or archived").choices(states))
        .build()
}

// Right-click a post › Apps › Log feat. Message menu commands have no description.
fn log_feat() -> Command {
    CommandBuilder::new(LOG_FEAT, "", CommandType::Message).build()
}
