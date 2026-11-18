//! /tutorial [guide]: how to use noxaDBRS, step by step, only to the caller. No story, just how.
//!
//! ```text
//!   /tutorial                 the players' guide: /me, picking who you play, /profile, Spirit Points
//!   /tutorial guide:staff     staff only: the mod log, the admin pages, the staff commands
//! ```
//!
//! Limits are read from the code (slots, amounts, sizes), so the guide says what the bot does.

use anyhow::Result;
use twilight_model::channel::message::Embed;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};

use super::ctx::{Ctx, Options};
use crate::domain::feats::NOTE_MAX;
use crate::domain::pictures::UPLOAD_MAX;
use crate::domain::spirit::{MEMO_MAX, SP_MAX};
use crate::services::characters::SLOT_MAX;

const ACCENT: u32 = 0xF0_7A_1E;
const MIB: usize = 1024 * 1024;
/// The option that picks a guide, and its values.
pub(crate) const GUIDE: &str = "guide";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Guide {
    Players,
    Staff,
}

impl Guide {
    pub(crate) const ALL: [Self; 2] = [Self::Players, Self::Staff];

    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Players => "players",
            Self::Staff => "staff",
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Players => "Players: your page, who you play, cards, Spirit Points",
            Self::Staff => "Staff: the mod log, the admin pages, staff commands",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.key() == key)
    }
}

fn section(name: &str, lines: &[String]) -> EmbedFieldBuilder {
    EmbedFieldBuilder::new(name, lines.join("\n"))
}

fn lines(items: &[&str]) -> Vec<String> {
    items.iter().map(|line| (*line).to_owned()).collect()
}

fn players() -> Vec<Embed> {
    let guide = EmbedBuilder::new()
        .title("📘 How to use noxaDBRS · players")
        .color(ACCENT)
        .description("Everything you need is on your page. Discord keeps one more command: /profile, to show a card in a channel.")
        .field(section(
            "1. Open your page: /me",
            &lines(&[
                "• Type `/me`. The bot DMs you a link.",
                "• DMs closed? The link shows here instead, only to you.",
                "• It works until 30 minutes pass without use, 4 hours at most. Run `/me` again for a new one; the old one stops.",
                "• Never share it: it opens your page without a password.",
            ]),
        ))
        .field(section(
            "2. On your page",
            &lines(&[
                "• **My characters**: each one as a card: picture, race, tier, title, bio, Dragon Balls, items, McGuffins.",
                "• **Play as <name>**: press it under a card to play that character. ✅ Playing marks the one you play.",
                "• **Spirit Points**: your balance and its last changes, with why.",
                "• **Players**: everyone with a character. Open one to see their cards.",
            ]),
        ))
        .field(section(
            "3. Show a card in a channel: /profile",
            &lines(&[
                "• `/profile`: the character you play.",
                "• `/profile character:<name>`: another of yours; the menu lists them.",
                "• `/profile player:@someone`: theirs; add `character:` to pick one.",
            ]),
        ))
        .field(section(
            "4. Characters, pictures, Spirit Points",
            &[
                format!("• Staff add your approved characters, up to {SLOT_MAX} slots."),
                "• Staff set pictures. With none, a card shows your Discord picture.".to_owned(),
                "• Spirit Points come only from staff; posting earns none.".to_owned(),
            ],
        ))
        .build();
    vec![guide]
}

fn staff() -> Vec<Embed> {
    let start = EmbedBuilder::new()
        .title("🛠️ How to use noxaDBRS · staff")
        .color(ACCENT)
        .description("Set the mod log once. Manage everything on the admin pages; use the commands for work in the flow of play.")
        .field(section(
            "1. The mod log, first",
            &lines(&[
                "• `/z_moderator_dbrs action:mod_log channel:#mod-log`",
                "• Nothing changes until it is set. Every change, from Discord or the web, is posted there, no pings.",
                "• Changes made on the web end with “· admin pages”.",
            ]),
        ))
        .field(section(
            "2. Open the admin pages: /z_moderator_admin",
            &lines(&[
                "• The bot DMs you a link: 30 minutes without use, 4 hours at most. A new one replaces the old.",
                "• Never share it. A player's /me link never opens these pages.",
            ]),
        ))
        .field(section(
            "3. Players",
            &[
                "• Everyone with Spirit Points or a character. **Open anyone** by user id or mention.".to_owned(),
                format!("• **Change Spirit Points**: give, take or set, up to {SP_MAX}, with a memo (up to {MEMO_MAX} characters)."),
                format!("• **Add character**: a slot from 1 to {SLOT_MAX} and the approved name."),
                "• The ledger: every change, by whom.".to_owned(),
            ],
        ))
        .field(section(
            "4. Characters and NPCs",
            &[
                "• **Sheet**: name, prefix, race, tier (0 to 99), title, bio, staff notes (players never see them). **Save sheet**.".to_owned(),
                format!(
                    "• **Picture**: a link (https), or **Upload**: PNG, JPEG, GIF or WebP up to {} MiB. Reload after a few seconds. Tick **Use their Discord icon** to drop it.",
                    UPLOAD_MAX / MIB
                ),
                "• **Carries**: give a McGuffin, or **Take back**.".to_owned(),
                format!("• **Feats**: **Log feat** (kind, counts as, a note up to {NOTE_MAX}, a post link), **Archive** with a reason, **Restore**."),
                "• **NPCs**: name and prefix, then their sheet like any character.".to_owned(),
            ],
        ))
        .field(section(
            "5. McGuffins and Dragon Balls",
            &lines(&[
                "• **Make a McGuffin**: name, emoji, held by one or many, what it is. Give, take back, **Retire**, **Restore**. Given to someone new, a one-holder McGuffin moves.",
                "• **Dragon Balls**, per set: **Roll needed** (1 to 100); **Search areas**: tick a category (all its text channels) or single channels; **Hide here** per ball; **Scatter** every loose ball over the areas. A held ball stays held.",
            ]),
        ))
        .build();

    let commands = EmbedBuilder::new()
        .title("🛠️ Staff commands, in the flow of play")
        .color(ACCENT)
        .description("Each picks its verb from `action`. Every change waits for the mod log.")
        .field(section(
            "Spirit Points: /z_moderator_sp",
            &lines(&[
                "• `action:give player:@x amount:5 memo:Won the tournament` (take and set alike).",
                "• `action:grant role:@Accepted amount:5 memo:Event`: everyone holding the role.",
                "• `action:history player:@x`: balance and last lines.",
            ]),
        ))
        .field(section(
            "Characters: /z_moderator_character",
            &lines(&["• `action:add player:@x slot:1 name:Captain Nova`"]),
        ))
        .field(section(
            "Feats: /z_moderator_feat",
            &lines(&[
                "• `action:add player:@x character:<menu> kind counts note [link]`",
                "• `action:edit|link|unlink|archive|restore feat:<menu>`, then what changes.",
                "• `action:list player:@x character:<menu> [state:archived]`",
                "• Right-click a post › Apps › **Log feat**: the post and its author's character filled in.",
            ]),
        ))
        .field(section(
            "When something fails",
            &lines(&[
                "• The reply says “That did not work (ref 123456)”. The bot's log has it: `journalctl -u noxadbrs -g 123456`.",
                "• A player trying a staff command gets “Staff only.”, and the log says who.",
            ]),
        ))
        .build();
    vec![start, commands]
}

pub(crate) async fn show(ctx: &Ctx<'_>, opts: &Options<'_>) -> Result<()> {
    let guide = opts.string(GUIDE).and_then(Guide::from_key).unwrap_or(Guide::Players);
    match guide {
        Guide::Staff if !ctx.is_staff() => ctx.deny().await,
        Guide::Staff => ctx.whisper_embeds(staff()).await,
        Guide::Players => ctx.whisper_embeds(players()).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use twilight_validate::embed::embed;

    // Discord's limit for all embeds of one message together.
    const MESSAGE_EMBEDS_MAX: usize = 6000;

    fn size(e: &Embed) -> usize {
        let fields: usize = e.fields.iter().map(|f| f.name.chars().count() + f.value.chars().count()).sum();
        e.title.as_deref().map_or(0, |t| t.chars().count()) + e.description.as_deref().map_or(0, |d| d.chars().count()) + fields
    }

    #[test]
    fn each_guide_fits_one_message() {
        for guide in [players(), staff()] {
            assert!(guide.iter().all(|e| embed(e).is_ok()));
            assert!(guide.iter().map(size).sum::<usize>() <= MESSAGE_EMBEDS_MAX);
        }
    }
}
