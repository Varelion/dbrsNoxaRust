//! Character slots, Bot1's way: browse them, and pick who you play.
//!
//! ```text
//!   /panel [player]      🎭 Character Panel: one field per slot, number buttons pick who you play
//!   /set character:<…>   play one of yours, by its menu
//!   .set1 … .set10       the quick way; the answer and the command go away after 7 seconds
//!
//!   ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//!       ⚔️  NOW PLAYING AS  ⚔️              every switch answers with this box
//!   ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! ```
//!
//! A panel's buttons work only for the player it belongs to, whoever else presses them.

use anyhow::Result;
use twilight_model::application::interaction::message_component::MessageComponentInteractionData;
use twilight_model::channel::message::Embed;
use twilight_model::channel::message::component::{ButtonStyle, Component};
use twilight_model::id::Id;
use twilight_model::id::marker::UserMarker;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder, EmbedFooterBuilder};

use super::characters::{menu, pick};
use super::ctx::{Ctx, Options};
use super::text::plain;
use super::widgets::{button, row};
use crate::domain::dragon_balls::ball_label;
use crate::domain::tutorial::Did;
use crate::services::characters::{Profile, SetActive};
use crate::store::characters::CharacterRow;

const PREFIX: &str = "noxadbrs|panel|";
const BLURPLE: u32 = 0x58_65_F2;
const GREEN: u32 = 0x57_F2_87;
const NUMBERS: [&str; 10] = ["1️⃣", "2️⃣", "3️⃣", "4️⃣", "5️⃣", "6️⃣", "7️⃣", "8️⃣", "9️⃣", "🔟"];
const PER_ROW: usize = 5;
const RULE: &str = "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━";
const NOTHING: &str = "—";
/// Ends a slot's field with an empty line, so slots stand apart. Discord trims plain blank lines.
const GAP: &str = "\n\u{200B}";

/// The box every switch answers with, as Bot1 draws it.
pub(crate) fn now_playing(name: &str, slot: i64) -> String {
    format!("```\n{RULE}\n    ⚔️  NOW PLAYING AS  ⚔️\n{RULE}\n\n    {name}\n    [ SLOT {slot} ]\n\n{RULE}\n```")
}

/// Whose panel is shown: yours, to play from, or someone else's, to look at.
#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Own,
    Other,
}

pub(crate) fn owns(custom_id: &str) -> bool {
    custom_id.starts_with(PREFIX)
}

fn number(slot: i64) -> &'static str {
    usize::try_from(slot - 1).ok().and_then(|i| NUMBERS.get(i)).copied().unwrap_or("#️⃣")
}

// One slot's lines: who they are, what they carry, how much they post, and whether they are played.
fn slot_lines(profile: &Profile, active: bool) -> String {
    let c = &profile.character;
    let tier = c.tier.map_or_else(|| "tier not set".to_owned(), |t| format!("Tier {t}"));
    let mut lines = vec![format!("{} · {tier}", plain(c.race.as_deref().unwrap_or("Race not set")))];
    if let Some(prefix) = c.prefix.as_deref().filter(|_| c.posting) {
        lines.push(format!("💬 `{prefix}:` · {} posts", c.posts));
    } else {
        lines.push(format!("{} posts", c.posts));
    }
    let carried: Vec<String> = profile
        .balls
        .iter()
        .map(|(set, star)| ball_label(*set, *star))
        .chain(profile.items.iter().map(|(item, n)| if *n > 1 { format!("{} ×{n}", item.label()) } else { item.label().to_owned() }))
        .chain(profile.mcguffins.iter().map(|(emoji, name)| format!("{emoji} {}", plain(name))))
        .collect();
    if !carried.is_empty() {
        lines.push(carried.join(" · "));
    }
    if active {
        lines.push("✅ **ACTIVE**".to_owned());
    }
    let text = lines.join("\n");
    let text = if text.is_empty() { NOTHING.to_owned() } else { text };
    format!("{text}{GAP}")
}

/// Whether a slot is the one being played.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Playing,
    Free,
}

// A slot's number button: green for the one played, and never disabled, so a player with one
// character still has something to press (the tutorial waits on it).
fn pick_button(player: Id<UserMarker>, id: i64, slot: i64, state: Slot) -> Component {
    let style = if state == Slot::Playing { ButtonStyle::Success } else { ButtonStyle::Primary };
    button(format!("{PREFIX}{player}|{id}"), &format!("{} {slot}", number(slot)), style)
}

fn render(ctx: &Ctx<'_>, player: Id<UserMarker>, view: View, color: u32) -> Result<Option<(Embed, Vec<Component>)>> {
    let characters = ctx.app.services.characters();
    let theirs = characters.of_player(player.get())?;
    if theirs.is_empty() {
        return Ok(None);
    }
    let active = characters.active(player.get())?.map(|c| c.id);
    let sp = ctx.app.services.spirit().balance(player.get())?;

    let description = match view {
        View::Own => "Select a character to set as active by clicking the corresponding number button below.".to_owned(),
        View::Other => format!("Only <@{player}> can pick who they play."),
    };
    let mut embed = EmbedBuilder::new().title("🎭 Character Panel").color(color).description(description);
    for character in &theirs {
        let Some(profile) = characters.profile(character.id)? else {
            continue;
        };
        let name = format!("{} Slot {}: {}", number(character.slot), character.slot, character.name);
        embed = embed.field(EmbedFieldBuilder::new(name, slot_lines(&profile, active == Some(character.id))));
    }
    let embed = embed.footer(EmbedFooterBuilder::new(format!("🔮 {sp} Spirit Points · .set<slot> switches too"))).validate()?.build();
    if view == View::Other {
        return Ok(Some((embed, Vec::new())));
    }

    let buttons: Vec<Component> = theirs
        .iter()
        .map(|c| {
            let state = if active == Some(c.id) { Slot::Playing } else { Slot::Free };
            pick_button(player, c.id, c.slot, state)
        })
        .collect();
    Ok(Some((embed, buttons.chunks(PER_ROW).map(|chunk| row(chunk.to_vec())).collect())))
}

pub(crate) async fn panel(ctx: &Ctx<'_>, opts: &Options<'_>) -> Result<()> {
    let Some(me) = ctx.user_id() else {
        return Ok(());
    };
    let player = opts.user("player").unwrap_or(me);
    let view = if player == me { View::Own } else { View::Other };
    match render(ctx, player, view, BLURPLE)? {
        Some((embed, components)) => ctx.panel(embed, components).await,
        None if view == View::Own => ctx.whisper("You have no characters yet. Staff add approved ones.").await,
        None => ctx.whisper(&format!("<@{player}> has no characters yet.")).await,
    }
}

/// A number pressed: that character is played, if the presser owns the panel.
pub(crate) async fn pressed(ctx: &Ctx<'_>, data: &MessageComponentInteractionData) -> Result<()> {
    let mut parts = data.custom_id.strip_prefix(PREFIX).unwrap_or_default().split('|');
    let (Some(owner), Some(id)) = (parts.next().and_then(|o| o.parse::<u64>().ok()).and_then(Id::new_checked), parts.next().and_then(|i| i.parse::<i64>().ok())) else {
        return Ok(());
    };
    let Some(me) = ctx.user_id() else {
        return Ok(());
    };
    if me != owner {
        return ctx.whisper(&format!("Only <@{owner}> can pick who they play.")).await;
    }
    let row = match ctx.app.services.characters().set_active(me.get(), id)? {
        SetActive::Set(row) => *row,
        SetActive::NotTheirs => return ctx.whisper("That character is not yours any more.").await,
    };
    tracing::info!(character = %row.name, "now playing, from the panel");
    let Some((embed, components)) = render(ctx, me, View::Own, GREEN)? else {
        return Ok(());
    };
    ctx.update(embed, components).await?;
    ctx.followup(&now_playing(&row.name, row.slot)).await?;
    super::tutorial::observe(ctx.app, me.get(), Did::PanelPick).await;
    Ok(())
}

/// /set character:<…>
pub(crate) async fn set(ctx: &Ctx<'_>, opts: &Options<'_>) -> Result<()> {
    let Some(me) = ctx.user_id() else {
        return Ok(());
    };
    let characters = ctx.app.services.characters();
    let mine = characters.of_player(me.get())?;
    let Some(picked) = pick(&mine, opts.string("character").unwrap_or_default()) else {
        return ctx.whisper("Pick one of your characters from the menu.").await;
    };
    match characters.set_active(me.get(), picked.id)? {
        SetActive::Set(row) => {
            ctx.whisper(&now_playing(&row.name, row.slot)).await?;
            super::tutorial::observe(ctx.app, me.get(), Did::PanelPick).await;
            Ok(())
        }
        SetActive::NotTheirs => ctx.whisper("Pick one of your characters from the menu.").await,
    }
}

/// /set's menu: the caller's own characters.
pub(crate) async fn complete(ctx: &Ctx<'_>, opts: &Options<'_>) -> Result<()> {
    let Some(me) = ctx.user_id() else {
        return ctx.choices(Vec::new()).await;
    };
    let typed = opts.focused().map(|(_, text)| text).unwrap_or_default();
    let mine = ctx.app.services.characters().of_player(me.get())?;
    ctx.choices(menu(&mine, typed)).await
}

/// ".set2", ". SET 2": the slot asked for; None when the message is not a .set.
pub(crate) fn dot_slot(content: &str) -> Option<Option<i64>> {
    let squashed: String = content.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase();
    let digits = squashed.strip_prefix(".set")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(digits.parse::<i64>().ok().filter(|n| (1..=crate::services::characters::SLOT_MAX).contains(n)))
}

/// What .set<slot> answers: the box, or why not.
pub(crate) fn dot_set(app: &super::app::App, player: u64, slot: Option<i64>) -> Result<(String, Option<CharacterRow>)> {
    let Some(slot) = slot else {
        return Ok(("Invalid character index.".to_owned(), None));
    };
    let characters = app.services.characters();
    let Some(row) = characters.of_player(player)?.into_iter().find(|c| c.slot == slot) else {
        return Ok((format!("You don't have a character at index {slot}"), None));
    };
    characters.set_active(player, row.id)?;
    Ok((now_playing(&row.name, row.slot), Some(row)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_set_reads_the_slot_loosely() {
        assert_eq!(dot_slot(".set1"), Some(Some(1)));
        assert_eq!(dot_slot(" . SET 10 "), Some(Some(10)));
        assert_eq!(dot_slot(".set11"), Some(None));
        assert_eq!(dot_slot(".set0"), Some(None));
        assert_eq!(dot_slot(".set"), None);
        assert_eq!(dot_slot(".settle"), None);
        assert_eq!(dot_slot(".set 1 now"), None);
    }

    #[test]
    fn the_played_slot_can_still_be_pressed() {
        // One character, already played: its button is the only one, and the tutorial waits on a press.
        let Component::Button(pick) = pick_button(Id::new(1), 7, 1, Slot::Playing) else {
            panic!("not a button");
        };
        assert!(!pick.disabled);
        assert_eq!(pick.style, ButtonStyle::Success);
    }

    #[test]
    fn the_box_names_the_character_and_slot() {
        let shown = now_playing("Captain Nova", 2);
        assert!(shown.contains("NOW PLAYING AS") && shown.contains("Captain Nova") && shown.contains("[ SLOT 2 ]"));
    }
}
