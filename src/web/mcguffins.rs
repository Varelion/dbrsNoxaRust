//! McGuffins: plot items and who holds them. Create, edit, give, take back, retire, restore; nothing
//! deletes one. A one-holder McGuffin given to someone new moves to them.
//!
//! ```text
//!   /mcguffins        every McGuffin, and a form to make one
//!   /mcguffins/<id>   edit it, its holders [take back], give it [character ▾], retire or restore
//! ```

use std::collections::HashMap;

use axum::extract::{Form, Path, State};
use axum::response::{Html, IntoResponse};
use serde::Deserialize;

use super::html::{self, Section, esc, options, plain};
use super::{NO_MOD_LOG, Page, Session, Web, done, expired, owner, refused};
use crate::domain::mcguffins::{DESCRIPTION_MAX, EMOJI_MAX, Holding, NAME_MAX};
use crate::domain::npcs::CAST;
use crate::services::mcguffins::{Draft, Given, McguffinView, Saved, Taken};
use crate::store::characters::CharacterRow;

/// The `back` value that sends a form back to the character's sheet; anything else, to the McGuffin.
pub(super) const BACK_CHARACTER: &str = "character";
const RETIRE: &str = "retire";
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
    let t = esc(&session.token);
    let rows: String = all
        .iter()
        .map(|m| {
            let holders: Vec<_> = m.holders.iter().map(|(id, name, _)| format!("<a href=\"/a/{t}/characters/{id}\">{}</a>", esc(name))).collect();
            let state = if m.retired { "<span class=\"tag\">retired</span>" } else { "" };
            format!(
                "<tr><td>{} <a href=\"/a/{t}/mcguffins/{}\">{}</a> {state}</td><td>{}</td><td>{}</td></tr>",
                esc(&m.emoji),
                m.id,
                esc(&m.name),
                esc(m.holding.label()),
                if holders.is_empty() { "<span class=\"dim\">nobody</span>".to_owned() } else { holders.join(", ") }
            )
        })
        .collect();
    let table = if all.is_empty() {
        "<p class=\"dim\">No McGuffins yet. Make the first below.</p>".to_owned()
    } else {
        format!("<div class=\"scroll\"><table><thead><tr><th>McGuffin</th><th>Held by</th><th>Holders</th></tr></thead><tbody>{rows}</tbody></table></div>")
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

    let body = format!(
        "<header><p><a href=\"/a/{t}/mcguffins\">← McGuffins</a></p><h1>{} {}</h1><p class=\"dim\">{}{}</p></header>\
         <div class=\"cols\"><section class=\"card\"><h2>Edit</h2><form method=\"post\" action=\"/a/{t}/mcguffins/{id}\" class=\"grid\">{}\
         <div><button type=\"submit\">Save</button></div></form></section>\
         <section class=\"card\"><h2>Holders</h2>{holders}{give}\
         <form method=\"post\" action=\"/a/{t}/mcguffins/{id}/retire\"><input type=\"hidden\" name=\"state\" value=\"{verb}\">\
         <button class=\"quiet\" type=\"submit\">{label}</button></form></section></div>",
        esc(&m.emoji),
        esc(&m.name),
        esc(m.holding.label()),
        if m.retired { " · retired" } else { "" },
        draft_fields(Some(&m)),
    );
    let told = web.told(&session);
    Ok(Html(html::page(&session.token, Section::Mcguffins, &m.name, told.as_ref(), &body)).into_response())
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
        Given::Done { moved_from } => {
            let from = moved_from.as_deref().map(|f| format!(", from **{}**", plain(f))).unwrap_or_default();
            web.audit(&format!("🔮 <@{}> gave **{}** to **{}** ({}){from}", session.staff, plain(&m.name), plain(&c.name), owner(c.player_id)));
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
