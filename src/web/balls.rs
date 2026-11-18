//! Dragon Balls: each set's search areas, the roll a search needs, and where its balls hide.
//!
//! ```text
//!   /balls          every set: held, hidden, not placed, roll
//!   /balls/<set>    roll [70] · balls: 1★ held by Nova · 2★ hidden in #wasteland [move ▾]
//!                   areas: ☑ category Earth (☑ #west-city ☐ #kame-house) · [scatter the loose balls]
//! ```
//!
//! A search counts in an area channel, or in any text channel of an area category.

use std::collections::{HashMap, HashSet};

use axum::extract::{Form, Path, State};
use axum::response::{Html, IntoResponse};
use serde::Deserialize;

use super::html::{self, Section, esc};
use super::{NO_MOD_LOG, Page, Web, done, expired, refused};
use crate::domain::dragon_balls::{AreaKind, BallSet, ROLL_MAX, ROLL_MIN};
use crate::services::balls::{Area, NeedSet, Placed, random_below, searchable};
use crate::services::guild::{Channel, ChannelKind};

const AREA_FIELD: &str = "area_";
const NO_SET: &str = "No such Dragon Ball set.";

fn channel_name(id: u64, channels: &[Channel]) -> String {
    channels.iter().find(|c| c.id == id).map_or_else(|| format!("#{id}"), |c| format!("#{}", c.name))
}

pub(super) async fn list(State(web): State<Web>, Path(token): Path<String>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let t = esc(&session.token);
    let mut rows = String::new();
    for set in BallSet::ALL {
        let view = web.services.balls().view(set)?;
        let held = view.balls.iter().filter(|b| b.holder.is_some()).count();
        let hidden = view.balls.iter().filter(|b| b.holder.is_none() && b.channel_id.is_some()).count();
        rows.push_str(&format!(
            "<tr><td><a href=\"/a/{t}/balls/{}\">{}</a></td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{held}</td><td class=\"num\">{hidden}</td><td class=\"num\">{}</td></tr>",
            set.key(),
            esc(set.label()),
            view.need,
            view.areas.len(),
            view.balls.len() - held - hidden
        ));
    }
    let body = format!(
        "<header><h1>Dragon Balls</h1><p class=\"dim\">Where each set can be found, and the 1d100 a search needs.</p></header>\
         <section class=\"card\"><div class=\"scroll\"><table><thead><tr><th>Set</th><th class=\"num\">Roll</th><th class=\"num\">Areas</th><th class=\"num\">Held</th>\
         <th class=\"num\">Hidden</th><th class=\"num\">Not placed</th></tr></thead><tbody>{rows}</tbody></table></div></section>"
    );
    let told = web.told(&session);
    Ok(Html(html::page(&session.token, Section::Balls, "Dragon Balls", told.as_ref(), &body)).into_response())
}

// Text channels grouped under their categories, as `<optgroup>`s; the one picked selected.
fn channel_options(channels: &[Channel], picked: Option<u64>) -> String {
    let option = |c: &Channel| {
        let selected = if Some(c.id) == picked { " selected" } else { "" };
        format!("<option value=\"{}\"{selected}>#{}</option>", c.id, esc(&c.name))
    };
    let loose: String = channels.iter().filter(|c| c.kind == ChannelKind::Text && c.parent.is_none()).map(option).collect();
    let grouped: String = channels
        .iter()
        .filter(|c| c.kind == ChannelKind::Category)
        .map(|category| {
            let inside: String = channels.iter().filter(|c| c.kind == ChannelKind::Text && c.parent == Some(category.id)).map(option).collect();
            if inside.is_empty() { String::new() } else { format!("<optgroup label=\"{}\">{inside}</optgroup>", esc(&category.name)) }
        })
        .collect();
    format!("<option value=\"\">Pick a channel</option>{loose}{grouped}")
}

// Every category with its text channels, each a checkbox; the set's areas ticked.
fn area_boxes(channels: &[Channel], areas: &[Area]) -> String {
    let ticked: HashSet<u64> = areas.iter().map(|a| a.channel_id).collect();
    let check = |c: &Channel, label: String| {
        let on = if ticked.contains(&c.id) { " checked" } else { "" };
        format!("<label class=\"check\"><input type=\"checkbox\" name=\"{AREA_FIELD}{}\" value=\"on\"{on}> {label}</label>", c.id)
    };
    let channel = |c: &Channel| check(c, format!("#{}", esc(&c.name)));
    let loose: String = channels.iter().filter(|c| c.kind == ChannelKind::Text && c.parent.is_none()).map(channel).collect();
    let categories: String = channels
        .iter()
        .filter(|c| c.kind == ChannelKind::Category)
        .map(|category| {
            let inside: String = channels.iter().filter(|c| c.kind == ChannelKind::Text && c.parent == Some(category.id)).map(channel).collect();
            let whole = check(category, format!("<b>{}</b>: all of it, new channels too", esc(&category.name)));
            format!("<fieldset><legend>{whole}</legend>{inside}</fieldset>")
        })
        .collect();
    let loose = if loose.is_empty() { String::new() } else { format!("<fieldset><legend>No category</legend>{loose}</fieldset>") };
    format!("<div class=\"tree\">{categories}{loose}</div>")
}

pub(super) async fn show(State(web): State<Web>, Path((token, key)): Path<(String, String)>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let Some(set) = BallSet::from_key(&key) else {
        return Ok(web.back(&session, "/balls", refused(NO_SET)));
    };
    let view = web.services.balls().view(set)?;
    let channels = web.guild.channels().await;
    let mut players: Vec<u64> = view.balls.iter().filter_map(|b| b.holder.as_ref().map(|(_, _, player)| *player)).collect();
    players.sort_unstable();
    players.dedup();
    let names: HashMap<u64, String> = web.guild.names(players).await;
    let t = esc(&session.token);

    let balls: String = view
        .balls
        .iter()
        .map(|b| {
            let star = b.star;
            let (place, form) = match (&b.holder, b.channel_id) {
                (Some((character, name, player)), _) => {
                    (format!("held by <a href=\"/a/{t}/characters/{character}\">{}</a> · {}", esc(name), html::person(*player, &names)), String::new())
                }
                (None, picked) => {
                    let place = picked.map_or_else(|| "<span class=\"dim\">not placed</span>".to_owned(), |c| format!("hidden in {}", esc(&channel_name(c, &channels))));
                    let form = format!(
                        "<form method=\"post\" action=\"/a/{t}/balls/{key}/place\" class=\"inline\"><input type=\"hidden\" name=\"star\" value=\"{star}\">\
                         <select name=\"channel\" aria-label=\"Where {star}★ hides\" required>{}</select><button class=\"quiet\" type=\"submit\">Hide here</button></form>",
                        channel_options(&channels, picked)
                    );
                    (place, form)
                }
            };
            format!("<tr><td><b>{star}★</b></td><td>{place}</td><td>{form}</td></tr>")
        })
        .collect();

    let searched = searchable(&view.areas, &channels).len();
    let areas_note = if channels.is_empty() { "<p class=\"notice bad\">Discord did not list the server's channels. Reload in a moment.</p>" } else { "" };
    let body = format!(
        "<header><p><a href=\"/a/{t}/balls\">← Dragon Balls</a></p><h1>🐉 {label}</h1><p class=\"dim\">Searches count in {searched} text channels.</p></header>\
         <div class=\"cols\"><section class=\"card\"><h2>Roll needed</h2>\
         <form method=\"post\" action=\"/a/{t}/balls/{key}/need\" class=\"row\"><label>A search succeeds on this or more, on 1d100\
         <input name=\"need\" type=\"number\" min=\"{ROLL_MIN}\" max=\"{ROLL_MAX}\" value=\"{need}\" required></label><button type=\"submit\">Save</button></form>\
         <h2>Balls</h2><div class=\"scroll\"><table><tbody>{balls}</tbody></table></div>\
         <form method=\"post\" action=\"/a/{t}/balls/{key}/scatter\"><button type=\"submit\">Scatter every loose ball over the areas</button></form>\
         <p class=\"dim small\">A held ball stays with its holder.</p></section>\
         <section class=\"card\"><h2>Search areas</h2>{areas_note}<form method=\"post\" action=\"/a/{t}/balls/{key}/areas\" class=\"tree\">{boxes}\
         <div><button type=\"submit\">Save areas</button></div></form></section></div>",
        label = esc(set.label()),
        need = view.need,
        boxes = area_boxes(&channels, &view.areas),
    );
    let told = web.told(&session);
    Ok(Html(html::page(&session.token, Section::Balls, set.label(), told.as_ref(), &body)).into_response())
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct NeedInput {
    need: String,
}

pub(super) async fn need(State(web): State<Web>, Path((token, key)): Path<(String, String)>, Form(input): Form<NeedInput>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let Some(set) = BallSet::from_key(&key) else {
        return Ok(web.back(&session, "/balls", refused(NO_SET)));
    };
    let path = format!("/balls/{key}");
    if !web.logged()? {
        return Ok(web.back(&session, &path, refused(NO_MOD_LOG)));
    }
    let need = input.need.trim().parse::<i64>().unwrap_or_default();
    let notice = match web.services.balls().set_need(set, need)? {
        NeedSet::Done { before, after } => {
            web.audit(&format!("🐉 <@{}> set {}'s roll to **{after}** (was {before})", session.staff, set.label()));
            done(format!("A search now needs {after} or more."))
        }
        NeedSet::OutOfRange => refused(format!("The roll runs from {ROLL_MIN} to {ROLL_MAX}.")),
    };
    Ok(web.back(&session, &path, notice))
}

fn area_line(area: &Area) -> String {
    match area.kind {
        AreaKind::Category => format!("<#{}> (category)", area.channel_id),
        AreaKind::Channel => format!("<#{}>", area.channel_id),
    }
}

pub(super) async fn areas(State(web): State<Web>, Path((token, key)): Path<(String, String)>, Form(input): Form<HashMap<String, String>>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let Some(set) = BallSet::from_key(&key) else {
        return Ok(web.back(&session, "/balls", refused(NO_SET)));
    };
    let path = format!("/balls/{key}");
    if !web.logged()? {
        return Ok(web.back(&session, &path, refused(NO_MOD_LOG)));
    }
    let channels = web.guild.channels().await;
    if channels.is_empty() {
        return Ok(web.back(&session, &path, refused("Discord did not list the server's channels; nothing changed. Try again in a moment.")));
    }

    // Only channels the server has: a ticked box is "area_<channel id>".
    let mut picked: Vec<Area> = input
        .keys()
        .filter_map(|name| name.strip_prefix(AREA_FIELD)?.parse::<u64>().ok())
        .filter_map(|id| channels.iter().find(|c| c.id == id))
        .map(|c| Area { channel_id: c.id, kind: if c.kind == ChannelKind::Category { AreaKind::Category } else { AreaKind::Channel } })
        .collect();
    picked.sort_by_key(|a| a.channel_id);

    let before = web.services.balls().set_areas(set, &picked)?;
    let added: Vec<_> = picked.iter().filter(|a| !before.contains(a)).map(|a| format!("+ {}", area_line(a))).collect();
    let removed: Vec<_> = before.iter().filter(|a| !picked.contains(a)).map(|a| format!("− {}", area_line(a))).collect();
    let changes = [added, removed].concat();
    if changes.is_empty() {
        return Ok(web.back(&session, &path, done("Saved. Nothing had changed.")));
    }
    web.audit(&format!("🐉 <@{}> changed {}'s search areas: {}", session.staff, set.label(), changes.join(" ")));
    Ok(web.back(&session, &path, done(format!("Search areas saved: {} changes.", changes.len()))))
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct PlaceInput {
    star: String,
    channel: String,
}

pub(super) async fn place(State(web): State<Web>, Path((token, key)): Path<(String, String)>, Form(input): Form<PlaceInput>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let Some(set) = BallSet::from_key(&key) else {
        return Ok(web.back(&session, "/balls", refused(NO_SET)));
    };
    let path = format!("/balls/{key}");
    if !web.logged()? {
        return Ok(web.back(&session, &path, refused(NO_MOD_LOG)));
    }
    let star = input.star.trim().parse::<i64>().unwrap_or_default();
    let channels = web.guild.channels().await;
    let Some(channel) = input.channel.trim().parse::<u64>().ok().filter(|id| channels.iter().any(|c| c.id == *id && c.kind == ChannelKind::Text)) else {
        return Ok(web.back(&session, &path, refused("Pick a text channel.")));
    };
    let notice = match web.services.balls().place(set, star, channel)? {
        Placed::Done => {
            web.audit(&format!("🐉 <@{}> hid {} {star}★ in <#{channel}>", session.staff, set.label()));
            done(format!("{star}★ hidden in {}.", channel_name(channel, &channels)))
        }
        Placed::Held { name } => refused(format!("{name} holds {star}★. Take it from them first.")),
        Placed::BadStar => refused("Pick a ball from 1★ to 7★."),
    };
    Ok(web.back(&session, &path, notice))
}

pub(super) async fn scatter(State(web): State<Web>, Path((token, key)): Path<(String, String)>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let Some(set) = BallSet::from_key(&key) else {
        return Ok(web.back(&session, "/balls", refused(NO_SET)));
    };
    let path = format!("/balls/{key}");
    if !web.logged()? {
        return Ok(web.back(&session, &path, refused(NO_MOD_LOG)));
    }
    let view = web.services.balls().view(set)?;
    let channels = searchable(&view.areas, &web.guild.channels().await);
    if channels.is_empty() {
        return Ok(web.back(&session, &path, refused(format!("{} has no text channels in its areas yet. Tick some first.", set.label()))));
    }
    let moved = web.services.balls().scatter(set, &channels, random_below)?;
    if moved.is_empty() {
        return Ok(web.back(&session, &path, refused("Every ball is held; nothing to scatter.")));
    }
    let spots: Vec<_> = moved.iter().map(|(star, channel)| format!("{star}★ <#{channel}>")).collect();
    web.audit(&format!("🐉 <@{}> scattered {}: {}", session.staff, set.label(), spots.join(" · ")));
    Ok(web.back(&session, &path, done(format!("{} balls scattered.", moved.len()))))
}
