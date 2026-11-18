//! A player's own page, from /mypage: what /panel, /set and /wallet did in Discord.
//!
//! ```text
//!   ┌ 🐉 Raging Spirit   My characters  Players ───────────────────────────┐
//!   │ 🔮 12 Spirit Points · day +5 → 12 Won the tournament                 │
//!   │ ┌ [picture] Captain Nova ┐ ┌ [picture] Nicolas ──────┐              │
//!   │ │ Earthling · Tier 9     │ │ …                        │              │
//!   │ │ Earth 1★ · 📡 Radar    │ │ [Play as Nicolas]        │              │
//!   │ │ ✅ Playing             │ └──────────────────────────┘              │
//!   │ └────────────────────────┘                                          │
//!   └──────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! Only what a player may see: no staff notes, no feats, no one else's ledger.

use std::collections::HashMap;

use axum::extract::{Form, Path, State};
use axum::response::{Html, IntoResponse, Redirect};
use serde::Deserialize;

use super::html::{self, Mine, esc, portrait};
use super::{Page, Web, done, expired, refused};
use crate::domain::dragon_balls::ball_label;
use crate::services::characters::{Profile, SetActive};
use crate::services::guild::{Person, Stored, picture_link};
use crate::services::links::Access;

const WALLET_LINES: usize = 10;
const NOT_YOURS: &str = "Pick one of your own characters.";

/// A player on a live /mypage link.
struct Me {
    token: String,
    player: u64,
}

impl Me {
    fn at(&self, path: &str) -> String {
        format!("/p/{}{path}", self.token)
    }
}

fn me(web: &Web, token: &str) -> Option<Me> {
    let player = web.services.links().open(token, Access::Player)?;
    Some(Me { token: token.to_owned(), player })
}

/// What a card offers under it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pick {
    /// Theirs, not played now: a button to play them.
    Play,
    /// Theirs, played now.
    Playing,
    /// Someone else's: nothing to press.
    Look,
}

// Every character of a player with what they carry, and the link to show for each picture.
async fn cards_of(web: &Web, player: u64) -> anyhow::Result<(Vec<Profile>, HashMap<u64, String>)> {
    let characters = web.services.characters();
    let mut profiles = Vec::new();
    for row in characters.of_player(player)? {
        if let Some(profile) = characters.profile(row.id)? {
            profiles.push(profile);
        }
    }
    let fresh = web.guild.fresh(profiles.iter().filter_map(|p| Stored::of(&p.character)).collect()).await;
    Ok((profiles, fresh))
}

fn card(m: Option<&Me>, profile: &Profile, picture: Option<&str>, pick: Pick) -> String {
    let c = &profile.character;
    let tier = c.tier.map_or_else(|| "tier not set".to_owned(), |t| format!("Tier {t}"));
    let mut lines = vec![format!("<p>{} · {tier}</p>", esc(c.race.as_deref().unwrap_or("Race not set")))];
    if let Some(title) = c.title.as_deref() {
        lines.push(format!("<p><i>{}</i></p>", esc(title)));
    }
    if let Some(bio) = c.bio.as_deref() {
        lines.push(format!("<p class=\"bio\">{}</p>", esc(bio)));
    }
    let carried: Vec<String> = profile
        .balls
        .iter()
        .map(|(set, star)| esc(&ball_label(*set, *star)))
        .chain(profile.items.iter().map(|(item, n)| if *n > 1 { format!("{} ×{n}", esc(item.label())) } else { esc(item.label()) }))
        .chain(profile.mcguffins.iter().map(|(emoji, name)| format!("{} {}", esc(emoji), esc(name))))
        .collect();
    let carried = if carried.is_empty() { "<span class=\"dim\">Carries nothing yet.</span>".to_owned() } else { carried.join(" · ") };

    let action = match (pick, m) {
        (Pick::Playing, _) => "<p><span class=\"tag\">✅ Playing</span></p>".to_owned(),
        (Pick::Play, Some(m)) => format!(
            "<form method=\"post\" action=\"{}\"><input type=\"hidden\" name=\"character\" value=\"{}\"><button type=\"submit\">Play as {}</button></form>",
            esc(&m.at("/play")),
            c.id,
            esc(&c.name)
        ),
        _ => String::new(),
    };
    let class = if pick == Pick::Playing { "card playing" } else { "card" };
    format!(
        "<article class=\"{class}\"><div class=\"head\">{}<div><h2>{}</h2><p class=\"dim small\">Slot {}</p></div></div>{}<p>{carried}</p>{action}</article>",
        portrait(picture, &c.name),
        esc(&c.name),
        c.slot,
        lines.concat()
    )
}

// A card's picture: an upload's fresh link, a typed link, else the player's Discord icon.
fn picture_for(profile: &Profile, fresh: &HashMap<u64, String>, owner: Option<&Person>) -> Option<String> {
    picture_link(&profile.character, fresh).or_else(|| owner.map(|p| p.icon.clone()))
}

pub(super) async fn home(State(web): State<Web>, Path(token): Path<String>) -> Page {
    let Some(m) = me(&web, &token) else {
        return Ok(expired());
    };
    let (profiles, fresh) = cards_of(&web, m.player).await?;
    let active = web.services.characters().active(m.player)?.map(|c| c.id);
    let people = web.guild.people(vec![m.player]).await;
    let owner = people.get(&m.player);

    let spirit = web.services.spirit();
    let balance = spirit.balance(m.player)?;
    let ledger: String = spirit
        .history(m.player, WALLET_LINES)?
        .iter()
        .map(|l| {
            let sign = if l.delta > 0 { "+" } else { "" };
            format!("<tr><td>{}</td><td class=\"num\">{sign}{}</td><td class=\"num\">{}</td><td>{}</td></tr>", html::day(l.at), l.delta, l.balance, esc(&l.memo))
        })
        .collect();
    let ledger = if ledger.is_empty() {
        "<p class=\"dim\">No changes yet. Staff give Spirit Points; posting earns none.</p>".to_owned()
    } else {
        format!("<div class=\"scroll\"><table><thead><tr><th>Day</th><th class=\"num\">Change</th><th class=\"num\">Balance</th><th>Why</th></tr></thead><tbody>{ledger}</tbody></table></div>")
    };

    let cards: String = profiles
        .iter()
        .map(|p| {
            let pick = if active == Some(p.character.id) { Pick::Playing } else { Pick::Play };
            card(Some(&m), p, picture_for(p, &fresh, owner).as_deref(), pick)
        })
        .collect();
    let cards = if cards.is_empty() { "<section class=\"card\"><p>You have no characters yet. Staff add approved ones.</p></section>".to_owned() } else { format!("<div class=\"cards\">{cards}</div>") };

    let name = owner.map_or_else(|| "Your page".to_owned(), |p| p.name.clone());
    let body = format!(
        "<header><h1>{}</h1><p class=\"dim\">Your characters, who you play, and your Spirit Points. Only you see this page.</p></header>\
         {cards}<section class=\"card\"><h2>🔮 {balance} Spirit Points</h2>{ledger}</section>",
        esc(&name)
    );
    let told = web.services.links().told(&m.token);
    Ok(Html(html::player_page(&m.token, Mine::Me, &name, told.as_ref(), &body)).into_response())
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct PlayInput {
    character: String,
}

pub(super) async fn play(State(web): State<Web>, Path(token): Path<String>, Form(input): Form<PlayInput>) -> Page {
    let Some(m) = me(&web, &token) else {
        return Ok(expired());
    };
    let id = input.character.trim().parse::<i64>().unwrap_or_default();
    let notice = match web.services.characters().set_active(m.player, id)? {
        SetActive::Set(row) => {
            tracing::info!(player = m.player, character = %row.name, "now playing, from the web");
            done(format!("You play {} now.", row.name))
        }
        SetActive::NotTheirs => refused(NOT_YOURS),
    };
    Ok(web.back_to(&m.token, &m.at(""), notice))
}

pub(super) async fn players(State(web): State<Web>, Path(token): Path<String>) -> Page {
    let Some(m) = me(&web, &token) else {
        return Ok(expired());
    };
    let players: Vec<_> = web.services.players().all()?.into_iter().filter(|p| p.characters > 0).collect();
    let sheets = web.services.sheets().all()?;
    let names = web.guild.names(players.iter().map(|p| p.player_id).collect()).await;

    let mut sorted: Vec<_> = players.iter().filter(|p| names.contains_key(&p.player_id)).collect();
    sorted.sort_by_key(|p| names.get(&p.player_id).map(|n| n.to_lowercase()));
    let rows: String = sorted
        .iter()
        .map(|p| {
            let theirs: Vec<_> = sheets.iter().filter(|c| c.player_id == p.player_id).map(|c| esc(&c.name)).collect();
            let you = if p.player_id == m.player { " <span class=\"tag\">you</span>" } else { "" };
            format!(
                "<tr><td><a href=\"{}\">{}</a>{you}</td><td>{}</td></tr>",
                esc(&m.at(&format!("/players/{}", p.player_id))),
                esc(names.get(&p.player_id).map_or("", String::as_str)),
                theirs.join(", ")
            )
        })
        .collect();
    let body = format!(
        "<header><h1>Players</h1><p class=\"dim\">Everyone on the server with a character.</p></header>\
         <section class=\"card\"><div class=\"scroll\"><table><thead><tr><th>Player</th><th>Characters</th></tr></thead><tbody>{rows}</tbody></table></div></section>"
    );
    let told = web.services.links().told(&m.token);
    Ok(Html(html::player_page(&m.token, Mine::Players, "Players", told.as_ref(), &body)).into_response())
}

pub(super) async fn player(State(web): State<Web>, Path((token, player)): Path<(String, u64)>) -> Page {
    let Some(m) = me(&web, &token) else {
        return Ok(expired());
    };
    if player == m.player {
        return Ok(Redirect::to(&m.at("")).into_response());
    }
    let (profiles, fresh) = cards_of(&web, player).await?;
    let people = web.guild.people(vec![player]).await;
    // Someone who left the server, or has no characters: nothing to show.
    let (Some(owner), false) = (people.get(&player), profiles.is_empty()) else {
        return Ok(web.back_to(&m.token, &m.at("/players"), refused("No characters to show for them.")));
    };
    let balance = web.services.spirit().balance(player)?;
    let cards: String = profiles.iter().map(|p| card(None, p, picture_for(p, &fresh, Some(owner)).as_deref(), Pick::Look)).collect();
    let body = format!(
        "<header><p><a href=\"{}\">← Players</a></p><h1>{}</h1><p class=\"dim\">🔮 {balance} Spirit Points · only they pick who they play.</p></header>\
         <div class=\"cards\">{cards}</div>",
        esc(&m.at("/players")),
        esc(&owner.name)
    );
    let told = web.services.links().told(&m.token);
    Ok(Html(html::player_page(&m.token, Mine::Players, &owner.name, told.as_ref(), &body)).into_response())
}
