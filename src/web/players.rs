//! Players: everyone with a wallet or a character. One player's page changes their Spirit Points and
//! adds their characters.
//!
//! ```text
//!   ┌ Nicolas · 157606850269937664 ─────────────────────────────┐
//!   │ 🔮 12 SP      [give ▾] [amount] [memo]  [Change]          │
//!   │ Ledger        day · +5 → 12 · memo · by staff             │
//!   │ Characters    slot · name · race · tier     [slot] [name] [Add] │
//!   └───────────────────────────────────────────────────────────┘
//! ```

use std::collections::HashMap;

use axum::extract::{Form, Path, Query, State};
use axum::response::{Html, IntoResponse, Redirect};
use serde::Deserialize;

use super::html::{self, Section, esc, options, or_dash, person, plain};
use super::{NO_MOD_LOG, Page, Web, done, expired, refused};
use crate::domain::spirit::{MEMO_MAX, SP_MAX, SpAction};
use crate::services::characters::{AddCharacter, NAME_MAX, SLOT_MAX};
use crate::services::spirit::{Change, Changed};

const SP: &str = "SP";
const LEDGER_LINES: usize = 25;
const NOT_AN_ID: &str = "That is not a Discord user id. In Discord: right-click them › Copy User ID.";
// What a page may change Spirit Points with; grant (by role) stays in Discord, which knows the roles.
const CHANGES: [SpAction; 3] = [SpAction::Give, SpAction::Take, SpAction::Set];

pub(super) async fn list(State(web): State<Web>, Path(token): Path<String>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let players = web.services.players().all()?;
    let sheets = web.services.sheets().all()?;
    let names = web.guild.names(players.iter().map(|p| p.player_id).collect()).await;

    // By name; players Discord does not know (they left) last, by id.
    let mut sorted: Vec<_> = players.iter().collect();
    sorted.sort_by_key(|p| (names.get(&p.player_id).map(|n| n.to_lowercase()).unwrap_or_else(|| "\u{10ffff}".to_owned()), p.player_id));

    let t = esc(&session.token);
    let rows: String = sorted
        .iter()
        .map(|p| {
            let theirs: Vec<_> = sheets
                .iter()
                .filter(|c| c.player_id == p.player_id)
                .map(|c| format!("<a href=\"/a/{t}/characters/{}\">{}</a>", c.id, esc(&c.name)))
                .collect();
            format!(
                "<tr><td><a href=\"/a/{t}/players/{id}\">{who}</a></td><td class=\"num\">{sp}</td><td>{chars}</td></tr>",
                id = p.player_id,
                who = person(p.player_id, &names),
                sp = p.sp,
                chars = if theirs.is_empty() { "<span class=\"dim\">none</span>".to_owned() } else { theirs.join(", ") },
            )
        })
        .collect();
    let table = if rows.is_empty() {
        "<p class=\"dim\">No players yet: they appear once they have Spirit Points or a character.</p>".to_owned()
    } else {
        format!("<div class=\"scroll\"><table><thead><tr><th>Player</th><th class=\"num\">{SP}</th><th>Characters</th></tr></thead><tbody>{rows}</tbody></table></div>")
    };

    let body = format!(
        "<header><h1>Players</h1><p class=\"dim\">{} with Spirit Points or a character.</p></header>\
         <section class=\"card\"><form method=\"get\" action=\"/a/{t}/players/find\" class=\"row\">\
         <label>Open anyone by user id or mention<input name=\"id\" placeholder=\"157606850269937664\" autocomplete=\"off\" required></label>\
         <button type=\"submit\">Open</button></form></section>\
         <section class=\"card\">{table}</section>",
        players.len()
    );
    let told = web.told(&session);
    Ok(Html(html::page(&session.token, Section::Players, "Players", told.as_ref(), &body)).into_response())
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct FindInput {
    id: String,
}

/// "157…", "<@157…>" or "<@!157…>": the id in it.
fn user_id_of(text: &str) -> Option<u64> {
    let digits = text.trim().trim_start_matches("<@").trim_start_matches('!').trim_end_matches('>');
    digits.parse::<u64>().ok().filter(|id| *id > 0)
}

pub(super) async fn find(State(web): State<Web>, Path(token): Path<String>, Query(input): Query<FindInput>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    match user_id_of(&input.id) {
        Some(id) => Ok(Redirect::to(&session.at(&format!("/players/{id}"))).into_response()),
        None => Ok(web.back(&session, "/players", refused(NOT_AN_ID))),
    }
}

pub(super) async fn show(State(web): State<Web>, Path((token, player)): Path<(String, u64)>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let spirit = web.services.spirit();
    let balance = spirit.balance(player)?;
    let ledger = spirit.history(player, LEDGER_LINES)?;
    let characters = web.services.characters();
    let theirs = characters.of_player(player)?;
    let active = characters.active(player)?.map(|c| c.id);

    let mut ids: Vec<u64> = std::iter::once(player).chain(ledger.iter().map(|l| l.staff_id)).collect();
    ids.sort_unstable();
    ids.dedup();
    let names: HashMap<u64, String> = web.guild.names(ids).await;
    let t = esc(&session.token);

    let changes: Vec<(String, String)> = CHANGES.iter().map(|a| (a.key().to_owned(), a.key().to_owned())).collect();
    let spirit_form = format!(
        "<form method=\"post\" action=\"/a/{t}/players/{player}/sp\" class=\"grid\">\
         <label>Change<select name=\"action\">{}</select></label>\
         <label>Amount<input name=\"amount\" type=\"number\" min=\"0\" max=\"{SP_MAX}\" required></label>\
         <label class=\"wide\">Memo: why, kept in the ledger and the mod log<input name=\"memo\" maxlength=\"{MEMO_MAX}\" required></label>\
         <div><button type=\"submit\">Change Spirit Points</button></div></form>",
        options(&changes, SpAction::Give.key())
    );

    let ledger_rows: String = ledger
        .iter()
        .map(|l| {
            let sign = if l.delta > 0 { "+" } else { "" };
            format!(
                "<tr><td>{}</td><td class=\"num\">{sign}{}</td><td class=\"num\">{}</td><td>{}</td><td>{}</td></tr>",
                html::day(l.at),
                l.delta,
                l.balance,
                esc(&l.memo),
                person(l.staff_id, &names)
            )
        })
        .collect();
    let ledger_table = if ledger.is_empty() {
        "<p class=\"dim\">No ledger lines yet.</p>".to_owned()
    } else {
        format!(
            "<div class=\"scroll\"><table><thead><tr><th>Day</th><th class=\"num\">Change</th><th class=\"num\">Balance</th><th>Memo</th><th>By</th></tr></thead>\
             <tbody>{ledger_rows}</tbody></table></div>"
        )
    };

    let character_rows: String = theirs
        .iter()
        .map(|c| {
            let playing = if active == Some(c.id) { " <span class=\"tag\">playing</span>" } else { "" };
            format!(
                "<tr><td class=\"num\">{}</td><td><a href=\"/a/{t}/characters/{}\">{}</a>{playing}</td><td>{}</td><td class=\"num\">{}</td></tr>",
                c.slot,
                c.id,
                esc(&c.name),
                or_dash(c.race.as_deref()),
                c.tier.map_or_else(|| "—".to_owned(), |tier| tier.to_string())
            )
        })
        .collect();
    let character_table = if theirs.is_empty() {
        "<p class=\"dim\">No characters yet.</p>".to_owned()
    } else {
        format!("<div class=\"scroll\"><table><thead><tr><th class=\"num\">Slot</th><th>Name</th><th>Race</th><th class=\"num\">Tier</th></tr></thead><tbody>{character_rows}</tbody></table></div>")
    };
    let next_slot = (1..=SLOT_MAX).find(|slot| !theirs.iter().any(|c| c.slot == *slot));
    let add_form = match next_slot {
        Some(slot) => format!(
            "<form method=\"post\" action=\"/a/{t}/players/{player}/characters\" class=\"row\">\
             <label>Slot<input name=\"slot\" type=\"number\" min=\"1\" max=\"{SLOT_MAX}\" value=\"{slot}\" required></label>\
             <label>Name of an approved character<input name=\"name\" maxlength=\"{NAME_MAX}\" required></label>\
             <button type=\"submit\">Add character</button></form>"
        ),
        None => format!("<p class=\"dim\">All {SLOT_MAX} slots are taken.</p>"),
    };

    let title = names.get(&player).cloned().unwrap_or_else(|| player.to_string());
    let body = format!(
        "<header class=\"head\"><div><p><a href=\"/a/{t}/players\">← Players</a></p><h1>{}</h1><p class=\"dim\">{player}</p></div></header>\
         <div class=\"cols\"><section class=\"card\"><h2>🔮 {balance} Spirit Points</h2>{spirit_form}</section>\
         <section class=\"card\"><h2>Characters</h2>{character_table}{add_form}</section></div>\
         <section class=\"card\"><h2>Ledger</h2>{ledger_table}</section>",
        esc(&title)
    );
    let told = web.told(&session);
    Ok(Html(html::page(&session.token, Section::Players, &title, told.as_ref(), &body)).into_response())
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct SpiritInput {
    action: String,
    amount: String,
    memo: String,
}

pub(super) async fn spirit(State(web): State<Web>, Path((token, player)): Path<(String, u64)>, Form(input): Form<SpiritInput>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let path = format!("/players/{player}");
    if !web.logged()? {
        return Ok(web.back(&session, &path, refused(NO_MOD_LOG)));
    }
    let Some(action) = SpAction::from_key(&input.action).filter(|a| CHANGES.contains(a)) else {
        return Ok(web.back(&session, &path, refused("Pick give, take or set.")));
    };
    let Ok(amount) = input.amount.trim().parse::<i64>() else {
        return Ok(web.back(&session, &path, refused(format!("Amount: a whole number from 0 to {SP_MAX}."))));
    };

    let staff = session.staff;
    let change = Change { action, player_id: player, amount, memo: &input.memo, staff_id: staff };
    let notice = match web.services.spirit().change(&change)? {
        Changed::Done { before, after } => {
            let what = match action {
                SpAction::Take => format!("took **{amount}** {SP} from <@{player}>: {before} → **{after}**"),
                SpAction::Set => format!("set <@{player}>'s {SP} to **{after}** (was {before})"),
                SpAction::Give | SpAction::Grant => format!("gave **{amount}** {SP} to <@{player}>: {before} → **{after}**"),
            };
            web.audit(&format!("🔮 <@{staff}> {what} · {}", plain(input.memo.trim())));
            done(format!("Spirit Points: {before} → {after}."))
        }
        Changed::NoMemo => refused("Write a memo: why."),
        Changed::BadAmount => refused(format!("Amount: a whole number from 1 to {SP_MAX} (set: from 0).")),
        Changed::TooLow { before } => refused(format!("They have {before} {SP}. Take {before} or less, or use set.")),
    };
    Ok(web.back(&session, &path, notice))
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct NewCharacterInput {
    slot: String,
    name: String,
}

pub(super) async fn add_character(State(web): State<Web>, Path((token, player)): Path<(String, u64)>, Form(input): Form<NewCharacterInput>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let path = format!("/players/{player}");
    if !web.logged()? {
        return Ok(web.back(&session, &path, refused(NO_MOD_LOG)));
    }
    let slot = input.slot.trim().parse::<i64>().unwrap_or_default();

    let staff = session.staff;
    let notice = match web.services.characters().add(player, slot, &input.name, staff)? {
        AddCharacter::Added(row) => {
            web.audit(&format!("👤 <@{staff}> added **{}** to <@{player}>'s slot {}", plain(&row.name), row.slot));
            done(format!("{} is in slot {}.", row.name, row.slot))
        }
        AddCharacter::SlotTaken => refused(format!("Slot {slot} is taken.")),
        AddCharacter::BadSlot => refused(format!("Slots run from 1 to {SLOT_MAX}.")),
        AddCharacter::NoName => refused("Write the character's name."),
    };
    Ok(web.back(&session, &path, notice))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_user_is_found_by_id_or_mention() {
        assert_eq!(user_id_of(" 157606850269937664 "), Some(157_606_850_269_937_664));
        assert_eq!(user_id_of("<@157606850269937664>"), Some(157_606_850_269_937_664));
        assert_eq!(user_id_of("<@!157606850269937664>"), Some(157_606_850_269_937_664));
        assert_eq!(user_id_of("Nicolas"), None);
        assert_eq!(user_id_of("0"), None);
    }
}
