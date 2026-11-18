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
//!
//! ```text
//!   Shop   🫘 Senzu Bean · 25 SP · Heals any wound at once.  [1] [Buy]   ─► 🎒 Inventory: 🫘 ×1
//! ```

use std::collections::HashMap;

use axum::extract::{Form, Path, State};
use axum::response::{Html, IntoResponse, Redirect};
use serde::Deserialize;

use super::html::{self, Mine, esc, portrait};
use super::sheets::{ExpressionInput, NameInput, expression_refusal, expressions_card};
use super::{Page, Web, done, expired, refused};
use crate::domain::dragon_balls::ball_label;
use crate::domain::shop::{BUY_MAX, most_owned, price_label};
use crate::domain::tutorial::Did;
use crate::services::characters::{Profile, SetActive};
use crate::services::sheets::{ExpressionSaved, Posting, PostingSaved};
use crate::services::guild::{Person, Stored, picture_link};
use crate::services::links::Access;
use crate::services::shop::{Bought, Shelf};
use crate::store::shop::ItemRow;

const WALLET_LINES: usize = 10;
const NOT_YOURS: &str = "Pick one of your own characters.";
/// Ends the mod log line of a purchase, so staff know where it came from.
const VIA_PAGE: &str = " · /mypage";

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

// A card's own settings, for its player: the prefix it posts with, and its expressions.
fn own_settings(web: &Web, m: &Me, profile: &Profile) -> anyhow::Result<String> {
    let c = &profile.character;
    let posting = format!(
        "<details><summary><b>Posting</b>: {}</summary><form method=\"post\" action=\"{}\" class=\"grid\">\
         <label>Prefix<input name=\"prefix\" value=\"{}\" maxlength=\"20\" placeholder=\"nova\" autocomplete=\"off\"></label>\
         <label class=\"check\"><input type=\"checkbox\" name=\"posting\" value=\"on\"{}> Posts by prefix</label>\
         <div><button type=\"submit\">Save posting</button></div></form></details>",
        c.prefix.as_deref().filter(|_| c.posting).map_or_else(|| "off".to_owned(), |p| format!("<code>{}: text</code>", esc(p))),
        esc(&m.at(&format!("/characters/{}/posting", c.id))),
        esc(c.prefix.as_deref().unwrap_or_default()),
        if c.posting { " checked" } else { "" },
    );
    let expressions = expressions_card(&m.at(&format!("/characters/{}/expressions", c.id)), c, &web.services.sheets().expressions(c.id)?);
    Ok(format!("{posting}<details><summary><b>Expressions</b></summary>{expressions}</details>"))
}

fn card(m: Option<&Me>, profile: &Profile, picture: Option<&str>, pick: Pick, settings: &str) -> String {
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
        "<article class=\"{class}\"><div class=\"head\">{}<div><h2>{}</h2><p class=\"dim small\">Slot {} · {} posts</p></div></div>{}<p>{carried}</p>{action}{settings}</article>",
        portrait(picture, &c.name),
        esc(&c.name),
        c.slot,
        c.posts,
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

    let mut cards = String::new();
    for p in &profiles {
        let pick = if active == Some(p.character.id) { Pick::Playing } else { Pick::Play };
        let settings = own_settings(&web, &m, p)?;
        cards.push_str(&card(Some(&m), p, picture_for(p, &fresh, owner).as_deref(), pick, &settings));
    }
    let cards = if cards.is_empty() { "<section class=\"card\"><p>You have no characters yet. Staff add approved ones.</p></section>".to_owned() } else { format!("<div class=\"cards\">{cards}</div>") };

    let name = owner.map_or_else(|| "Your page".to_owned(), |p| p.name.clone());
    let inventory = inventory_card(&web, &m)?;
    let body = format!(
        "<header><h1>{}</h1><p class=\"dim\">Your characters, who you play, and your Spirit Points. Only you see this page.</p></header>\
         {cards}<div class=\"cols\"><section class=\"card\"><h2>🔮 {balance} Spirit Points</h2>{ledger}</section>{inventory}</div>",
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
    let cards: String = profiles.iter().map(|p| card(None, p, picture_for(p, &fresh, Some(owner)).as_deref(), Pick::Look, "")).collect();
    let body = format!(
        "<header><p><a href=\"{}\">← Players</a></p><h1>{}</h1><p class=\"dim\">🔮 {balance} Spirit Points · only they pick who they play.</p></header>\
         <div class=\"cards\">{cards}</div>",
        esc(&m.at("/players")),
        esc(&owner.name)
    );
    let told = web.services.links().told(&m.token);
    Ok(Html(html::player_page(&m.token, Mine::Players, &owner.name, told.as_ref(), &body)).into_response())
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct PostingInput {
    prefix: String,
    posting: String,
}

pub(super) async fn posting(State(web): State<Web>, Path((token, id)): Path<(String, i64)>, Form(input): Form<PostingInput>) -> Page {
    let Some(m) = me(&web, &token) else {
        return Ok(expired());
    };
    let posting = if input.posting == "on" { Posting::ByPrefix } else { Posting::Off };
    let notice = match web.services.sheets().set_posting(m.player, id, &input.prefix, posting)? {
        PostingSaved::Done(row) => {
            tracing::info!(player = m.player, character = %row.name, prefix = ?row.prefix, "posting set on the web");
            if row.prefix.is_some() && row.posting {
                web.guild.did(m.player, Did::PrefixSet);
            }
            match row.prefix.as_deref().filter(|_| row.posting) {
                Some(prefix) => done(format!("{} posts with {prefix}: text", row.name)),
                None => done(format!("{} posts by prefix no more.", row.name)),
            }
        }
        PostingSaved::NotYours => refused(NOT_YOURS),
        PostingSaved::BadPrefix(reason) => refused(format!("Prefix: {reason}.")),
        PostingSaved::PrefixTaken(other) => refused(format!("Your {other} uses that prefix already.")),
    };
    Ok(web.back_to(&m.token, &m.at(""), notice))
}

pub(super) async fn add_expression(State(web): State<Web>, Path((token, id)): Path<(String, i64)>, Form(input): Form<ExpressionInput>) -> Page {
    let Some(m) = me(&web, &token) else {
        return Ok(expired());
    };
    if web.services.sheets().owned(m.player, id)?.is_none() {
        return Ok(web.back_to(&m.token, &m.at(""), refused(NOT_YOURS)));
    }
    let notice = match web.services.sheets().save_expression(id, &input.name, &input.url, &input.color)? {
        ExpressionSaved::Done(e) => done(format!("Saved the {} expression.", e.name)),
        other => refused(expression_refusal(&other)),
    };
    Ok(web.back_to(&m.token, &m.at(""), notice))
}

pub(super) async fn remove_expression(State(web): State<Web>, Path((token, id)): Path<(String, i64)>, Form(input): Form<NameInput>) -> Page {
    let Some(m) = me(&web, &token) else {
        return Ok(expired());
    };
    if web.services.sheets().owned(m.player, id)?.is_none() {
        return Ok(web.back_to(&m.token, &m.at(""), refused(NOT_YOURS)));
    }
    let notice = if web.services.sheets().remove_expression(id, &input.name)? { done("Removed.") } else { refused("No such expression.") };
    Ok(web.back_to(&m.token, &m.at(""), notice))
}

// What they own from the shop.
fn inventory_card(web: &Web, m: &Me) -> anyhow::Result<String> {
    let owned = web.services.shop().inventory(m.player)?;
    let list = if owned.is_empty() {
        format!("<p class=\"dim\">Nothing yet. Buy in the <a href=\"{}\">Shop</a>.</p>", esc(&m.at("/shop")))
    } else {
        let rows: String = owned.iter().map(|(item, n)| format!("<tr><td>{}</td><td class=\"num\">×{n}</td></tr>", esc(&item.label()))).collect();
        format!("<div class=\"scroll\"><table><tbody>{rows}</tbody></table></div>")
    };
    Ok(format!("<section class=\"card\"><h2>🎒 Inventory</h2>{list}</section>"))
}

// One item on the shelf: what it is, its price, how many they have, and a way to buy it.
fn shelf_card(m: &Me, item: &ItemRow, owned: i64) -> String {
    let most = most_owned(item.price);
    let room = (most - owned).min(BUY_MAX);
    let had = if owned > 0 { format!(" · you have {owned}") } else { String::new() };
    let buy = if room > 0 {
        format!(
            "<form method=\"post\" action=\"{}\" class=\"inline\"><input name=\"count\" type=\"number\" min=\"1\" max=\"{room}\" value=\"1\" aria-label=\"How many\" style=\"width:5em\"><button type=\"submit\">Buy</button></form>",
            esc(&m.at(&format!("/shop/{}/buy", item.id)))
        )
    } else {
        "<p class=\"dim small\">You have all you may.</p>".to_owned()
    };
    format!(
        "<article class=\"card\"><div class=\"head\"><span class=\"emoji\" aria-hidden=\"true\">{}</span><div><h2>{}</h2><p class=\"dim small\">{}{had}</p></div></div><p>{}</p>{buy}</article>",
        esc(&item.emoji),
        esc(&item.name),
        price_label(item.price),
        esc(&item.description)
    )
}

pub(super) async fn shop(State(web): State<Web>, Path(token): Path<String>) -> Page {
    let Some(m) = me(&web, &token) else {
        return Ok(expired());
    };
    let shop = web.services.shop();
    let owned = shop.inventory(m.player)?;
    let count = |id: i64| owned.iter().find(|(i, _)| i.id == id).map_or(0, |(_, n)| *n);
    let shelf: String = shop.items(Shelf::Listed)?.iter().map(|item| shelf_card(&m, item, count(item.id))).collect();
    let shelf = if shelf.is_empty() { "<section class=\"card\"><p>The shop is empty for now.</p></section>".to_owned() } else { format!("<div class=\"cards\">{shelf}</div>") };
    let balance = web.services.spirit().balance(m.player)?;

    let body = format!(
        "<header><h1>Shop</h1><p class=\"dim\">🔮 {balance} Spirit Points to spend. What you buy goes to your inventory.</p></header>{shelf}{}",
        inventory_card(&web, &m)?
    );
    let told = web.services.links().told(&m.token);
    Ok(Html(html::player_page(&m.token, Mine::Shop, "Shop", told.as_ref(), &body)).into_response())
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct BuyInput {
    count: String,
}

pub(super) async fn buy(State(web): State<Web>, Path((token, id)): Path<(String, i64)>, Form(input): Form<BuyInput>) -> Page {
    let Some(m) = me(&web, &token) else {
        return Ok(expired());
    };
    let count = input.count.trim().parse::<i64>().unwrap_or_default();
    let notice = match web.services.shop().buy(m.player, id, count)? {
        Bought::Done { item, count, cost, after, owned } => {
            tracing::info!(player = m.player, item = %item.name, count, cost, "bought in the shop");
            web.guild.audit(format!("🛒 <@{}> bought {count} × {} for {} ({after} SP left){VIA_PAGE}", m.player, html::plain(&item.label()), price_label(cost)));
            web.guild.did(m.player, Did::Bought);
            done(format!("Bought {count} × {}. You have {owned}; {after} Spirit Points left.", item.label()))
        }
        Bought::Missing => refused("That item is not in the shop any more."),
        Bought::BadCount => refused(format!("Buy from 1 to {BUY_MAX} at a time.")),
        Bought::TooMany { owned, most } => refused(format!("You may have {most} of it; you have {owned}.")),
        Bought::TooPoor { balance, cost } => refused(format!("That costs {cost} SP; you have {balance}.")),
    };
    Ok(web.back_to(&m.token, &m.at("/shop"), notice))
}
