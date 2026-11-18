//! /profile [player] [character]: a character's card. Public.
//!
//! ```text
//!   ┌ Captain Nova ──────────────────────────┐ [picture: the character's, else the owner's icon]
//!   │ Earthling · Tier 9                      │
//!   │ EDF Commander                           │
//!   │ Player  @captainnovavt                  │
//!   │ Dragon Balls  Earth 1★   Items 📡 Dragon Radar │
//!   └ slot 1 ─────────────────────────────────┘
//! ```

use anyhow::Result;
use twilight_model::application::interaction::application_command::CommandData;
use twilight_model::id::Id;
use twilight_model::id::marker::{GuildMarker, UserMarker};
use twilight_model::util::ImageHash;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder, EmbedFooterBuilder, ImageSource};

use super::characters::{menu, pick};
use super::ctx::{Ctx, Options};
use super::guild;
use super::text::{cut, plain};
use crate::domain::dragon_balls::ball_label;
use crate::services::characters::Profile;
use crate::store::characters::CharacterRow;

const ACCENT: u32 = 0xF0_7A_1E;
const CDN: &str = "https://cdn.discordapp.com";
const ICON_SIZE: u16 = 256;
// Discord's default icons, picked from the account id, for an account without its own.
const DEFAULT_ICONS: u64 = 6;
const SNOWFLAKE_TIME_SHIFT: u32 = 22;
const DESCRIPTION_MAX: usize = 1000;
const NOTHING: &str = "—";

/// An account's icon as the server shows it: its server picture, else its own, else Discord's default.
pub(crate) fn icon_url(guild: Id<GuildMarker>, user: Id<UserMarker>, server: Option<ImageHash>, own: Option<ImageHash>) -> String {
    let ext = |hash: &ImageHash| if hash.is_animated() { "gif" } else { "png" };
    if let Some(hash) = server {
        return format!("{CDN}/guilds/{guild}/users/{user}/avatars/{hash}.{}?size={ICON_SIZE}", ext(&hash));
    }
    if let Some(hash) = own {
        return format!("{CDN}/avatars/{user}/{hash}.{}?size={ICON_SIZE}", ext(&hash));
    }
    format!("{CDN}/embed/avatars/{}.png", (user.get() >> SNOWFLAKE_TIME_SHIFT) % DEFAULT_ICONS)
}

// The player whose card is asked for, and their icon hashes: the named one, else the caller.
fn whose(ctx: &Ctx<'_>, opts: &Options<'_>, data: &CommandData) -> Option<(Id<UserMarker>, Option<ImageHash>, Option<ImageHash>)> {
    let Some(named) = opts.user("player") else {
        return ctx.caller_icons();
    };
    let resolved = data.resolved.as_ref();
    let own = resolved.and_then(|r| r.users.get(&named)).and_then(|u| u.avatar);
    let server = resolved.and_then(|r| r.members.get(&named)).and_then(|m| m.avatar);
    Some((named, server, own))
}

fn card(profile: &Profile, picture: String) -> Result<twilight_model::channel::message::Embed> {
    let c = &profile.character;
    let race = c.race.as_deref().unwrap_or("Race not set");
    let tier = c.tier.map_or_else(|| "tier not set".to_owned(), |t| format!("Tier {t}"));
    let mut description = format!("{} · {tier}", plain(race));
    if let Some(title) = c.title.as_deref() {
        description.push_str(&format!("\n*{}*", plain(title)));
    }
    if let Some(bio) = c.bio.as_deref() {
        description.push_str(&format!("\n\n{}", plain(bio)));
    }

    let balls: Vec<_> = profile.balls.iter().map(|(set, star)| ball_label(*set, *star)).collect();
    let items: Vec<_> = profile
        .items
        .iter()
        .map(|(item, n)| if *n > 1 { format!("{} ×{n}", item.label()) } else { item.label().to_owned() })
        .chain(profile.mcguffins.iter().map(|(emoji, name)| format!("{emoji} {}", plain(name))))
        .collect();
    let listed = |list: Vec<String>| if list.is_empty() { NOTHING.to_owned() } else { list.join("\n") };

    Ok(EmbedBuilder::new()
        .title(c.name.clone())
        .color(ACCENT)
        .description(cut(&description, DESCRIPTION_MAX))
        .thumbnail(ImageSource::url(picture)?)
        .field(EmbedFieldBuilder::new("Player", format!("<@{}>", c.player_id)).inline())
        .field(EmbedFieldBuilder::new("Dragon Balls", listed(balls)).inline())
        .field(EmbedFieldBuilder::new("Items", listed(items)).inline())
        .field(EmbedFieldBuilder::new("Posts", c.posts.to_string()).inline())
        .footer(EmbedFooterBuilder::new(format!("Slot {}", c.slot)))
        .build())
}

// Which character: the one picked, else the one they play, else their first.
fn chosen(characters: &[CharacterRow], picked: Option<&str>, active: Option<i64>) -> Option<i64> {
    if let Some(picked) = picked {
        return pick(characters, picked).map(|c| c.id);
    }
    active.filter(|id| characters.iter().any(|c| c.id == *id)).or_else(|| characters.first().map(|c| c.id))
}

pub(crate) async fn show(ctx: &Ctx<'_>, opts: &Options<'_>, data: &CommandData) -> Result<()> {
    let Some((player, server, own)) = whose(ctx, opts, data) else {
        return Ok(());
    };
    let characters = ctx.app.services.characters();
    let theirs = characters.of_player(player.get())?;
    let active = characters.active(player.get())?.map(|c| c.id);
    let Some(id) = chosen(&theirs, opts.string("character"), active) else {
        return ctx.whisper(&format!("<@{player}> has no characters yet.")).await;
    };
    let Some(profile) = characters.profile(id)? else {
        return ctx.whisper("That character is gone.").await;
    };

    // An uploaded picture's link expires: a fresh one is fetched when it nears that.
    let picture = match guild::picture_of(ctx.app, &profile.character).await {
        Some(picture) => picture,
        None => icon_url(ctx.app.guild_id, player, server, own),
    };
    ctx.show(card(&profile, picture)?).await
}

/// The character menu: the named player's, else the caller's.
pub(crate) async fn complete(ctx: &Ctx<'_>, opts: &Options<'_>) -> Result<()> {
    let typed = opts.focused().map(|(_, text)| text).unwrap_or_default();
    let Some(player) = opts.user("player").or_else(|| ctx.user_id()) else {
        return ctx.choices(Vec::new()).await;
    };
    let theirs = ctx.app.services.characters().of_player(player.get())?;
    ctx.choices(menu(&theirs, typed)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUILD: u64 = 1_556_437_391_693_324_349;
    const USER: u64 = 410_879_158_533_619_712;

    #[test]
    fn the_server_picture_wins_then_the_own_icon_then_the_default() {
        let (guild, user) = (Id::new(GUILD), Id::new(USER));
        let still: ImageHash = "a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7".parse().unwrap();
        let moving: ImageHash = "a_a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7".parse().unwrap();

        assert_eq!(icon_url(guild, user, Some(still), Some(moving)), format!("{CDN}/guilds/{GUILD}/users/{USER}/avatars/{still}.png?size=256"));
        assert_eq!(icon_url(guild, user, None, Some(moving)), format!("{CDN}/avatars/{USER}/{moving}.gif?size=256"));
        assert!(icon_url(guild, user, None, None).starts_with(&format!("{CDN}/embed/avatars/")));
    }

    #[test]
    fn the_card_names_the_active_character_unless_one_is_picked() {
        let row = |id, slot, name: &str| CharacterRow { id, player_id: USER, slot, name: name.to_owned(), race: None, tier: None, title: None, bio: None, picture_url: None, picture_message: None, prefix: None, notes: None, posting: true, posts: 0 };
        let theirs = [row(1, 1, "Noxa"), row(2, 2, "Yuan")];
        assert_eq!(chosen(&theirs, None, Some(2)), Some(2));
        assert_eq!(chosen(&theirs, None, None), Some(1));
        assert_eq!(chosen(&theirs, Some("noxa"), Some(2)), Some(1));
        assert_eq!(chosen(&theirs, Some("Goku"), None), None);
        assert_eq!(chosen(&[], None, None), None);
    }
}
