//! McGuffins: plot items and who holds them. Create, edit, give, take back, retire, restore; nothing
//! deletes one. A one-holder McGuffin given to someone new moves to them.
//!
//! ```text
//!   /mcguffins        every McGuffin, where it lies, and a form to make one
//!   /mcguffins/<id>   edit it, where it lies [world ▾ | channel ▾ | random] [re-roll], its holders
//!                     [take back], give it [character ▾], retire or restore
//! ```
//!
//! Where it lies is picked from the server's channels as they are when the page opens, and checked
//! again on save: a world is a category ticked on the Worlds page; random rolls one of their text channels.

use std::collections::HashMap;

use axum::extract::{Form, Path, State};
use axum::response::{Html, IntoResponse};
use serde::Deserialize;

use super::balls::channel_options;
use super::html::{self, Section, esc, options, plain};
use super::{NO_MOD_LOG, Page, Session, Web, done, expired, owner, refused};
use crate::domain::mcguffins::{DESCRIPTION_MAX, EMOJI_MAX, Holding, NAME_MAX, Place};
use crate::domain::npcs::CAST;
use crate::services::balls::random_below;
use crate::services::guild::{Channel, ChannelKind};
use crate::services::mcguffins::{Draft, Given, McguffinView, Placed, Placing, Saved, Taken};
use crate::services::worlds::roleplay;
use crate::store::characters::CharacterRow;

/// The `back` value that sends a form back to the character's sheet; anything else, to the McGuffin.
pub(super) const BACK_CHARACTER: &str = "character";
const RETIRE: &str = "retire";
// The place form's choices, as posted.
const NOWHERE: &str = "nowhere";
const WORLD: &str = "world";
const CHANNEL: &str = "channel";
const RANDOM: &str = "random";
const GONE: &str = "That McGuffin is gone.";

/// Where a give or take sends staff back to.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Back {
    Character,
    Mcguffin,
}

impl Back {
    fn of(value: &str) -> Self {
        if value == BACK_CHARACTER { Self::Character } else { Self::Mcguffin }
    }

    fn path(self, mcguffin: i64, character: i64) -> String {
        match self {
            Self::Character => format!("/characters/{character}"),
            Self::Mcguffin => format!("/mcguffins/{mcguffin}"),
        }
    }
}

/// Where it lies, for a page: named from the server's channels now; a channel since deleted says so.
fn where_shown(place: Option<Place>, channels: &[Channel]) -> String {
    let named = |id: u64| channels.iter().find(|c| c.id == id).map(|c| esc(&c.name));
    let gone = |id: u64| format!("<span class=\"bad\">a channel the server no longer has ({id})</span>");
    match place {
        None => "<span class=\"dim\">nowhere</span>".to_owned(),
        Some(Place::World(id)) => named(id).map_or_else(|| gone(id), |n| format!("🌍 world <b>{n}</b>")),
        Some(Place::Channel(id)) => named(id).map_or_else(|| gone(id), |n| format!("#{n}")),
        Some(Place::Random(id)) => named(id).map_or_else(|| gone(id), |n| format!("🎲 #{n}, rolled")),
    }
}

/// Where it lies, in plain words for a notice: "in world Earth", "at random in #ruins".
fn where_named(place: Option<Place>, channels: &[Channel]) -> String {
    let named = |id: u64| channels.iter().find(|c| c.id == id).map_or_else(|| id.to_string(), |c| c.name.clone());
    match place {
        None => "nowhere".to_owned(),
        Some(Place::World(id)) => format!("in world {}", named(id)),
        Some(Place::Channel(id)) => format!("in #{}", named(id)),
        Some(Place::Random(id)) => format!("at random in #{}", named(id)),
    }
}

/// Where it lies, for a mod log line.
fn where_logged(place: Option<Place>) -> String {
    match place {
        None => "nowhere".to_owned(),
        Some(Place::World(id)) => format!("in world <#{id}>"),
        Some(Place::Channel(id)) => format!("in <#{id}>"),
        Some(Place::Random(id)) => format!("at random in <#{id}>"),
    }
}

fn holdings() -> Vec<(String, String)> {
    [Holding::One, Holding::Many].iter().map(|h| (h.key().to_owned(), h.label().to_owned())).collect()
}

fn refusal(outcome: &Saved) -> &'static str {
    match outcome {
        Saved::Done(_) => "",
        Saved::NoName => "Write a name.",
        Saved::NameTaken => "Another McGuffin has that name.",
        Saved::NotFound => GONE,
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct DraftInput {
    name: String,
    emoji: String,
    description: String,
    holding: String,
}

impl DraftInput {
    fn draft(&self) -> Draft<'_> {
        Draft { name: &self.name, emoji: &self.emoji, description: &self.description, holding: Holding::from_key(&self.holding).unwrap_or(Holding::Many) }
    }
}

// The fields, shared by the make and edit forms.
fn draft_fields(m: Option<&McguffinView>) -> String {
    let holding = m.map_or(Holding::Many, |m| m.holding);
    format!(
        "<label>Name<input name=\"name\" value=\"{}\" maxlength=\"{NAME_MAX}\" required></label>\
         <label>Emoji<input name=\"emoji\" value=\"{}\" maxlength=\"{EMOJI_MAX}\" placeholder=\"🔮\"></label>\
         <label>Held by<select name=\"holding\">{}</select></label>\
         <label class=\"wide\">What it is<textarea name=\"description\" maxlength=\"{DESCRIPTION_MAX}\">{}</textarea></label>",
        esc(m.map_or("", |m| m.name.as_str())),
        esc(m.map_or("", |m| m.emoji.as_str())),
        options(&holdings(), holding.key()),
        esc(m.map_or("", |m| m.description.as_str())),
    )
}

pub(super) async fn list(State(web): State<Web>, Path(token): Path<String>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let all = web.services.mcguffins().all()?;
    let channels = if all.iter().any(|m| m.place.is_some()) { web.guild.channels().await } else { Vec::new() };
    let t = esc(&session.token);
    let rows: String = all
        .iter()
        .map(|m| {
            let holders: Vec<_> = m.holders.iter().map(|(id, name, _)| format!("<a href=\"/a/{t}/characters/{id}\">{}</a>", esc(name))).collect();
            let state = if m.retired { "<span class=\"tag\">retired</span>" } else { "" };
            format!(
                "<tr><td>{} <a href=\"/a/{t}/mcguffins/{}\">{}</a> {state}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                esc(&m.emoji),
                m.id,
                esc(&m.name),
                esc(m.holding.label()),
                where_shown(m.place, &channels),
                if holders.is_empty() { "<span class=\"dim\">nobody</span>".to_owned() } else { holders.join(", ") }
            )
        })
        .collect();
    let table = if all.is_empty() {
        "<p class=\"dim\">No McGuffins yet. Make the first below.</p>".to_owned()
    } else {
        format!("<div class=\"scroll\"><table><thead><tr><th>McGuffin</th><th>Held by</th><th>Lies</th><th>Holders</th></tr></thead><tbody>{rows}</tbody></table></div>")
    };
    let body = format!(
        "<header><h1>McGuffins</h1><p class=\"dim\">Plot items. Retiring hides one; nothing deletes it.</p></header>\
         <section class=\"card\">{table}</section>\
         <section class=\"card\"><h2>Make a McGuffin</h2><form method=\"post\" action=\"/a/{t}/mcguffins\" class=\"grid\">{}\
         <div><button type=\"submit\">Make McGuffin</button></div></form></section>",
        draft_fields(None)
    );
    let told = web.told(&session);
    Ok(Html(html::page(&session.token, Section::Mcguffins, "McGuffins", told.as_ref(), &body)).into_response())
}

pub(super) async fn create(State(web): State<Web>, Path(token): Path<String>, Form(input): Form<DraftInput>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    if !web.logged()? {
        return Ok(web.back(&session, "/mcguffins", refused(NO_MOD_LOG)));
    }
    match web.services.mcguffins().create(&input.draft(), session.staff)? {
        Saved::Done(m) => {
            web.audit(&format!("🔮 <@{}> made McGuffin **{}** {} ({})", session.staff, plain(&m.name), m.emoji, m.holding.label().to_lowercase()));
            Ok(web.back(&session, &format!("/mcguffins/{}", m.id), done(format!("{} made.", m.name))))
        }
        other => Ok(web.back(&session, "/mcguffins", refused(refusal(&other)))),
    }
}

// "Captain Nova (Nicolas)", "King Yenma (NPC)": a character in a menu.
fn labelled(c: &CharacterRow, names: &HashMap<u64, String>) -> String {
    let whose = if c.player_id == CAST { "NPC".to_owned() } else { names.get(&c.player_id).cloned().unwrap_or_else(|| c.player_id.to_string()) };
    format!("{} ({whose})", c.name)
}

pub(super) async fn show(State(web): State<Web>, Path((token, id)): Path<(String, i64)>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let Some(m) = web.services.mcguffins().get(id)? else {
        return Ok(web.back(&session, "/mcguffins", refused(GONE)));
    };
    let everyone = web.services.sheets().all()?;
    let mut owners: Vec<u64> = everyone.iter().map(|c| c.player_id).filter(|p| *p != CAST).collect();
    owners.sort_unstable();
    owners.dedup();
    let names = web.guild.names(owners).await;
    let t = esc(&session.token);

    let holders: String = m
        .holders
        .iter()
        .map(|(character, name, player)| {
            let whose = if *player == CAST { "NPC".to_owned() } else { html::person(*player, &names) };
            format!(
                "<tr><td><a href=\"/a/{t}/characters/{character}\">{}</a></td><td>{whose}</td><td><form method=\"post\" action=\"/a/{t}/mcguffins/{id}/take\">\
                 <input type=\"hidden\" name=\"character\" value=\"{character}\"><button class=\"quiet\" type=\"submit\">Take back</button></form></td></tr>",
                esc(name)
            )
        })
        .collect();
    let holders = if m.holders.is_empty() {
        "<p class=\"dim\">Nobody holds it.</p>".to_owned()
    } else {
        format!("<div class=\"scroll\"><table><thead><tr><th>Character</th><th>Player</th><th></th></tr></thead><tbody>{holders}</tbody></table></div>")
    };

    let mut givable: Vec<_> = everyone.iter().filter(|c| !m.holders.iter().any(|(holder, _, _)| holder == &c.id)).collect();
    givable.sort_by_key(|c| c.name.to_lowercase());
    let choices: Vec<(String, String)> = givable.iter().map(|c| (c.id.to_string(), labelled(c, &names))).collect();
    let give = match (m.retired, choices.is_empty()) {
        (true, _) => "<p class=\"dim\">Retired: restore it to give it out.</p>".to_owned(),
        (false, true) => "<p class=\"dim\">Every character holds it.</p>".to_owned(),
        (false, false) => {
            let moves = if m.holding == Holding::One { " It moves from whoever holds it now." } else { "" };
            format!(
                "<form method=\"post\" action=\"/a/{t}/mcguffins/{id}/give\" class=\"row\"><label>Give to<select name=\"character\">{}</select></label>\
                 <button type=\"submit\">Give</button></form><p class=\"dim small\">{}{moves}</p>",
                options(&choices, ""),
                esc(m.holding.label())
            )
        }
    };
    let (verb, label) = if m.retired { ("restore", "Restore: back in play") } else { (RETIRE, "Retire: hide it from cards") };
    let place = place_card(&t, &m, &web.guild.channels().await, &web.services.worlds().all()?);

    let body = format!(
        "<header><p><a href=\"/a/{t}/mcguffins\">← McGuffins</a></p><h1>{} {}</h1><p class=\"dim\">{}{}</p></header>\
         <div class=\"cols\"><section class=\"card\"><h2>Edit</h2><form method=\"post\" action=\"/a/{t}/mcguffins/{id}\" class=\"grid\">{}\
         <div><button type=\"submit\">Save</button></div></form></section>\
         <section class=\"card\"><h2>Holders</h2>{holders}{give}\
         <form method=\"post\" action=\"/a/{t}/mcguffins/{id}/retire\"><input type=\"hidden\" name=\"state\" value=\"{verb}\">\
         <button class=\"quiet\" type=\"submit\">{label}</button></form></section></div>{place}",
        esc(&m.emoji),
        esc(&m.name),
        esc(m.holding.label()),
        if m.retired { " · retired" } else { "" },
        draft_fields(Some(&m)),
    );
    let told = web.told(&session);
    Ok(Html(html::page(&session.token, Section::Mcguffins, &m.name, told.as_ref(), &body)).into_response())
}

// Where it lies, and the form that moves it: built from the server's channels and worlds now.
fn place_card(t: &str, m: &McguffinView, channels: &[Channel], worlds: &[u64]) -> String {
    let id = m.id;
    let world_choices: Vec<(String, String)> = channels
        .iter()
        .filter(|c| c.kind == ChannelKind::Category && worlds.contains(&c.id))
        .map(|c| (c.id.to_string(), c.name.clone()))
        .collect();
    let pool = roleplay(worlds, channels).len();
    let current_world = match m.place {
        Some(Place::World(w)) => w.to_string(),
        _ => String::new(),
    };
    let current_channel = match m.place {
        Some(Place::Channel(c)) => Some(c),
        _ => None,
    };
    let mode = m.place.map_or(WORLD, |p| p.key());
    let modes = [
        (WORLD, "In a world: somewhere in a category".to_owned()),
        (CHANNEL, "In one channel".to_owned()),
        (RANDOM, format!("At random: one of the {pool} roleplay channels")),
        (NOWHERE, "Nowhere".to_owned()),
    ];
    let modes: Vec<(String, String)> = modes.into_iter().map(|(k, l)| (k.to_owned(), l)).collect();
    let worlds_note = if world_choices.is_empty() {
        format!("<p class=\"notice bad\">No worlds yet: tick them on the <a href=\"/a/{t}/worlds\">Worlds</a> page.</p>")
    } else {
        String::new()
    };
    let reroll = if matches!(m.place, Some(Place::Random(_))) {
        format!(
            "<form method=\"post\" action=\"/a/{t}/mcguffins/{id}/place\"><input type=\"hidden\" name=\"mode\" value=\"{RANDOM}\">\
             <button class=\"quiet\" type=\"submit\">🎲 Re-roll</button></form>"
        )
    } else {
        String::new()
    };
    let listing = if channels.is_empty() { "<p class=\"notice bad\">Discord did not list the server's channels. Reload in a moment.</p>" } else { "" };
    format!(
        "<section class=\"card\"><h2>Where it lies</h2><p>{}</p>{listing}{worlds_note}\
         <form method=\"post\" action=\"/a/{t}/mcguffins/{id}/place\" class=\"grid\">\
         <label>Where<select name=\"mode\">{}</select></label>\
         <label>World, for “in a world”<select name=\"world\"><option value=\"\">Pick a world</option>{}</select></label>\
         <label>Channel, for “in one channel”<select name=\"channel\">{}</select></label>\
         <div><button type=\"submit\">Save place</button></div></form>{reroll}\
         <p class=\"dim small\">Read from the server's channels now. Given to a character, a one-holder McGuffin leaves its place.</p></section>",
        where_shown(m.place, channels),
        options(&modes, mode),
        options(&world_choices, &current_world),
        channel_options(channels, current_channel),
    )
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct PlaceInput {
    mode: String,
    world: String,
    channel: String,
}

pub(super) async fn place(State(web): State<Web>, Path((token, id)): Path<(String, i64)>, Form(input): Form<PlaceInput>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let path = format!("/mcguffins/{id}");
    if !web.logged()? {
        return Ok(web.back(&session, &path, refused(NO_MOD_LOG)));
    }
    let picked = |text: &str| text.trim().parse::<u64>().unwrap_or_default();
    let placing = match input.mode.as_str() {
        NOWHERE => Placing::Nowhere,
        WORLD => Placing::World(picked(&input.world)),
        CHANNEL => Placing::Channel(picked(&input.channel)),
        RANDOM => Placing::Random,
        _ => return Ok(web.back(&session, &path, refused("Pick where it lies."))),
    };
    let channels = web.guild.channels().await;
    if channels.is_empty() && placing != Placing::Nowhere {
        return Ok(web.back(&session, &path, refused("Discord did not list the server's channels; nothing changed. Try again in a moment.")));
    }
    let Some(m) = web.services.mcguffins().get(id)? else {
        return Ok(web.back(&session, "/mcguffins", refused(GONE)));
    };
    let worlds = web.services.worlds().all()?;
    let notice = match web.services.mcguffins().place(id, placing, &channels, &worlds, random_below)? {
        Placed::Done { before, after, pool } => {
            let rolled = if pool > 0 { format!(" (of {pool} roleplay channels)") } else { String::new() };
            web.audit(&format!("🔮 <@{}> put McGuffin **{}** {}{rolled} (was {})", session.staff, plain(&m.name), where_logged(after), where_logged(before)));
            done(format!("{} now lies {}.", m.name, where_named(after, &channels)))
        }
        Placed::NotFound => return Ok(web.back(&session, "/mcguffins", refused(GONE))),
        Placed::NotAWorld => refused("Pick a world: a category ticked on the Worlds page, that the server still has."),
        Placed::NotAChannel => refused("Pick a text channel the server has."),
        Placed::NoRoleplay => refused("No world has a text channel to roll from. Tick worlds first."),
    };
    Ok(web.back(&session, &path, notice))
}

pub(super) async fn edit(State(web): State<Web>, Path((token, id)): Path<(String, i64)>, Form(input): Form<DraftInput>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let path = format!("/mcguffins/{id}");
    if !web.logged()? {
        return Ok(web.back(&session, &path, refused(NO_MOD_LOG)));
    }
    let notice = match web.services.mcguffins().edit(id, &input.draft())? {
        Saved::Done(m) => {
            web.audit(&format!("🔮 <@{}> edited McGuffin **{}**", session.staff, plain(&m.name)));
            done("Saved.")
        }
        other => refused(refusal(&other)),
    };
    Ok(web.back(&session, &path, notice))
}

/// Gives a McGuffin to a character, from either page.
pub(super) fn give_to(web: &Web, session: &Session, mcguffin: i64, character: i64, back: Back) -> Page {
    let path = back.path(mcguffin, character);
    if !web.logged()? {
        return Ok(web.back(session, &path, refused(NO_MOD_LOG)));
    }
    let (Some(m), Some(c)) = (web.services.mcguffins().get(mcguffin)?, web.services.sheets().get(character)?) else {
        return Ok(web.back(session, &path, refused("Pick a McGuffin and a character.")));
    };
    let notice = match web.services.mcguffins().give(mcguffin, character, session.staff)? {
        Given::Done { moved_from, left } => {
            let from = moved_from.as_deref().map(|f| format!(", from **{}**", plain(f))).unwrap_or_default();
            let lay = left.map(|p| format!("; it no longer lies {}", where_logged(Some(p)))).unwrap_or_default();
            web.audit(&format!("🔮 <@{}> gave **{}** to **{}** ({}){from}{lay}", session.staff, plain(&m.name), plain(&c.name), owner(c.player_id)));
            done(format!("{} now holds {}.", c.name, m.name))
        }
        Given::NotFound => refused(GONE),
        Given::Retired => refused("That McGuffin is retired. Restore it first."),
        Given::NoCharacter => refused("That character is gone."),
        Given::AlreadyHeld => refused(format!("{} holds it already.", c.name)),
    };
    Ok(web.back(session, &path, notice))
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct HolderInput {
    character: String,
    back: String,
}

pub(super) async fn give(State(web): State<Web>, Path((token, id)): Path<(String, i64)>, Form(input): Form<HolderInput>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let character = input.character.trim().parse::<i64>().unwrap_or_default();
    give_to(&web, &session, id, character, Back::of(&input.back))
}

pub(super) async fn take(State(web): State<Web>, Path((token, id)): Path<(String, i64)>, Form(input): Form<HolderInput>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let character = input.character.trim().parse::<i64>().unwrap_or_default();
    let path = Back::of(&input.back).path(id, character);
    if !web.logged()? {
        return Ok(web.back(&session, &path, refused(NO_MOD_LOG)));
    }
    let (Some(m), Some(c)) = (web.services.mcguffins().get(id)?, web.services.sheets().get(character)?) else {
        return Ok(web.back(&session, &path, refused("Pick a McGuffin and a character.")));
    };
    let notice = match web.services.mcguffins().take(id, character)? {
        Taken::Done => {
            web.audit(&format!("🔮 <@{}> took **{}** back from **{}** ({})", session.staff, plain(&m.name), plain(&c.name), owner(c.player_id)));
            done(format!("Took {} back from {}.", m.name, c.name))
        }
        Taken::NotHeld => refused(format!("{} does not hold it.", c.name)),
    };
    Ok(web.back(&session, &path, notice))
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct RetireInput {
    state: String,
}

pub(super) async fn retire(State(web): State<Web>, Path((token, id)): Path<(String, i64)>, Form(input): Form<RetireInput>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let path = format!("/mcguffins/{id}");
    if !web.logged()? {
        return Ok(web.back(&session, &path, refused(NO_MOD_LOG)));
    }
    let retiring = input.state == RETIRE;
    let Some(m) = web.services.mcguffins().set_retired(id, retiring, session.staff)? else {
        return Ok(web.back(&session, "/mcguffins", refused(GONE)));
    };
    let verb = if retiring { "retired" } else { "restored" };
    web.audit(&format!("🔮 <@{}> {verb} McGuffin **{}**", session.staff, plain(&m.name)));
    Ok(web.back(&session, &path, done(format!("{} {verb}.", m.name))))
}
