//! Characters: staff add approved ones to a player's slot. Players pick who they play on their /mypage page.

use anyhow::Result;

use super::ctx::{Ctx, Options};
use super::modlog::{self, NO_MOD_LOG};
use super::text::{capitalized, cut, plain};
use crate::services::characters::{AddCharacter, SLOT_MAX};
use crate::store::characters::CharacterRow;

const MENU_LIMIT: usize = 25;
const CHOICE_MAX: usize = 100;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CharacterSub {
    Add,
}

impl CharacterSub {
    pub(crate) const ALL: [Self; 1] = [Self::Add];

    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Add => "add",
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Add => "Add: an approved character to a player's slot",
        }
    }
}

/// A character of the player's, by a menu's id, its slot or its name.
pub(crate) fn pick<'a>(characters: &'a [CharacterRow], picked: &str) -> Option<&'a CharacterRow> {
    let typed = picked.trim().to_lowercase();
    characters.iter().find(|c| c.id.to_string() == typed || c.slot.to_string() == typed || c.name.to_lowercase() == typed)
}

/// A menu of the player's characters whose name holds the typed text: "1 · Itami".
pub(crate) fn menu(characters: &[CharacterRow], typed: &str) -> Vec<(String, String)> {
    let typed = typed.to_lowercase();
    characters
        .iter()
        .filter(|c| c.name.to_lowercase().contains(&typed))
        .take(MENU_LIMIT)
        .map(|c| (cut(&format!("{} · {}", c.slot, c.name), CHOICE_MAX), c.id.to_string()))
        .collect()
}

pub(crate) async fn admin(ctx: &Ctx<'_>, opts: &Options<'_>) -> Result<()> {
    if !ctx.is_staff() {
        return ctx.deny().await;
    }
    ctx.defer().await?;
    if opts.sub() != Some(CharacterSub::Add.key()) {
        return ctx.edit("Pick an action.").await;
    }
    let Some(log) = modlog::required(ctx.app)? else {
        return ctx.edit(NO_MOD_LOG).await;
    };
    let (Some(staff), Some(player), Some(slot)) = (ctx.user_id(), opts.user("player"), opts.integer("slot")) else {
        return ctx.edit("Name a player and a slot.").await;
    };
    let name = opts.string("name").unwrap_or_default();

    let added = match ctx.app.services.characters().add(player.get(), slot, name, staff.get())? {
        AddCharacter::Added(row) => *row,
        AddCharacter::SlotTaken => return ctx.edit(&format!("<@{player}>'s slot {slot} is taken.")).await,
        AddCharacter::BadSlot => return ctx.edit(&format!("Slots run from 1 to {SLOT_MAX}.")).await,
        AddCharacter::NoName => return ctx.edit("Write the character's name.").await,
    };

    let what = format!("added **{}** to <@{player}>'s slot {}", plain(&added.name), added.slot);
    modlog::post(ctx.app, log, &format!("👤 <@{staff}> {what}{}", modlog::where_of(ctx.channel_id()))).await;
    ctx.edit(&format!("👤 {}.", capitalized(&what))).await
}
