//! Characters and NPCs: every sheet, one sheet with what it carries and its feats, a picture upload.
//!
//! ```text
//!   ┌ [picture] Captain Nova · Earthling · Tier 9 · Nicolas ───────────────┐
//!   │ Sheet       name prefix race tier title picture bio notes  [Save]    │
//!   │ Picture     [file]  [Upload] ─► shrunk, sent to the pictures channel     │
//!   │ Carries     Dragon Balls, items, McGuffins   [give ▾] [take back]    │
//!   │ Feats       active [archive], archived [restore], [log a feat]       │
//!   └──────────────────────────────────────────────────────────────────────┘
//! ```

use std::collections::HashMap;

use axum::extract::{Form, Path, State};
use axum::response::{Html, IntoResponse};
use serde::Deserialize;

use super::html::{self, Section, esc, options, or_dash, person, plain, portrait};
use super::mcguffins::{BACK_CHARACTER, Back, give_to};
use super::uploads::{UPLOAD_SCRIPT, expression_form, picture_form};
use super::{NO_MOD_LOG, Page, Session, Web, done, expired, owner, refused};
use crate::domain::dragon_balls::ball_label;
use crate::domain::feats::{FeatCounts, FeatKind, FeatState, NOTE_MAX, REASON_MAX};
use crate::domain::links::parse_link;
use crate::domain::npcs::CAST;
use crate::services::characters::NAME_MAX;
use crate::services::feats::{Added, FeatView, Moved, NewFeat, PostRef};
use crate::services::guild::{fresh_or, picture_link};
use crate::services::links::Notice;
use crate::services::sheets::{ExpressionSaved, PictureChoice, Posting, SheetForm, SheetSaved, Target, changed};
use crate::store::expressions::ExpressionRow;
use crate::store::characters::CharacterRow;

const GONE: &str = "That character is gone.";
const ICON_ON: &str = "on";

/// A sheet as the form holds it: from the database, or as typed when a save was refused.
#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct SheetInput {
    name: String,
    prefix: String,
    race: String,
    tier: String,
    title: String,
    bio: String,
    picture_url: String,
    icon: String,
    notes: String,
    posting: String,
}

impl SheetInput {
    fn of(row: &CharacterRow) -> Self {
        let text = |value: Option<&str>| value.unwrap_or_default().to_owned();
        Self {
            name: row.name.clone(),
            prefix: text(row.prefix.as_deref()),
            race: text(row.race.as_deref()),
            tier: row.tier.map(|t| t.to_string()).unwrap_or_default(),
            title: text(row.title.as_deref()),
            bio: text(row.bio.as_deref()),
            // An upload's link is Discord's and expires: the field holds only a link staff typed.
            picture_url: if row.picture_message.is_some() { String::new() } else { text(row.picture_url.as_deref()) },
            icon: String::new(),
            notes: text(row.notes.as_deref()),
            posting: if row.posting { ICON_ON.to_owned() } else { String::new() },
        }
    }

    fn form(&self) -> SheetForm {
        SheetForm {
            name: self.name.clone(),
            prefix: self.prefix.clone(),
            race: self.race.clone(),
            tier: self.tier.clone(),
            title: self.title.clone(),
            bio: self.bio.clone(),
            picture_url: self.picture_url.clone(),
            picture: if self.icon == ICON_ON { PictureChoice::Icon } else { PictureChoice::Link },
            notes: self.notes.clone(),
            posting: if self.posting == ICON_ON { Posting::ByPrefix } else { Posting::Off },
        }
    }
}

/// Why a save was refused, in a sentence.
fn refusal(outcome: &SheetSaved) -> String {
    match outcome {
        SheetSaved::Done { .. } => String::new(),
        SheetSaved::NotFound => GONE.to_owned(),
        SheetSaved::NoName => "Write a name.".to_owned(),
        SheetSaved::BadPrefix(reason) => format!("Prefix: {reason}."),
        SheetSaved::PrefixTaken(owner) => format!("That prefix is {owner}'s."),
        SheetSaved::BadTier => "Tier: a whole number from 0 to 99, or blank.".to_owned(),
        SheetSaved::BadPicture => "Picture: an https:// link to an image, or blank.".to_owned(),
    }
}

// The cast are NPCs, everyone else a player's.
fn whose(row: &CharacterRow, names: &HashMap<u64, String>) -> String {
    if row.player_id == CAST {
        return "<span class=\"tag\">NPC</span>".to_owned();
    }
    person(row.player_id, names)
}

pub(super) async fn list(State(web): State<Web>, Path(token): Path<String>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let all: Vec<_> = web.services.sheets().all()?.into_iter().filter(|c| c.player_id != CAST).collect();
    let mut owners: Vec<u64> = all.iter().map(|c| c.player_id).collect();
    owners.dedup();
    let names = web.guild.names(owners).await;

    let mut sorted: Vec<_> = all.iter().collect();
    sorted.sort_by_key(|c| c.name.to_lowercase());
    let t = esc(&session.token);
    let rows: String = sorted
        .iter()
        .map(|c| {
            format!(
                "<tr><td><a href=\"/a/{t}/characters/{}\">{}</a></td><td><a href=\"/a/{t}/players/{}\">{}</a></td><td class=\"num\">{}</td><td>{}</td><td class=\"num\">{}</td><td>{}</td></tr>",
                c.id,
                esc(&c.name),
                c.player_id,
                person(c.player_id, &names),
                c.slot,
                or_dash(c.race.as_deref()),
                c.tier.map_or_else(|| "—".to_owned(), |tier| tier.to_string()),
                or_dash(c.prefix.as_deref()),
            )
        })
        .collect();
    let table = if rows.is_empty() {
        "<p class=\"dim\">No characters yet. Add one on a player's page.</p>".to_owned()
    } else {
        format!(
            "<div class=\"scroll\"><table><thead><tr><th>Character</th><th>Player</th><th class=\"num\">Slot</th><th>Race</th><th class=\"num\">Tier</th><th>Prefix</th></tr></thead>\
             <tbody>{rows}</tbody></table></div>"
        )
    };
    let body = format!(
        "<header><h1>Characters</h1><p class=\"dim\">{} players' characters. NPCs have <a href=\"/a/{t}/npcs\">their own page</a>.</p></header>\
         <section class=\"card\">{table}</section>",
        all.len()
    );
    let told = web.told(&session);
    Ok(Html(html::page(&session.token, Section::Characters, "Characters", told.as_ref(), &body)).into_response())
}

pub(super) async fn npcs(State(web): State<Web>, Path(token): Path<String>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let cast = web.services.sheets().npcs()?;
    let fresh = web.guild.fresh(cast.iter().filter_map(|c| c.picture_url.clone()).collect()).await;
    let t = esc(&session.token);
    let rows: String = cast
        .iter()
        .map(|npc| {
            let picture = picture_link(npc, &fresh)
                .filter(|url| url.starts_with("https://"))
                .map(|url| format!("<img class=\"pic\" src=\"{}\" alt=\"\">", esc(&url)))
                .unwrap_or_default();
            format!(
                "<tr><td>{picture}</td><td><a href=\"/a/{t}/characters/{}\">{}</a></td><td><code>{}</code></td><td>{}</td><td class=\"num\">{}</td><td>{}</td></tr>",
                npc.id,
                esc(&npc.name),
                esc(&npc.prefix.as_deref().map(|p| format!("{p}:")).unwrap_or_else(|| "—".to_owned())),
                or_dash(npc.race.as_deref()),
                npc.tier.map_or_else(|| "—".to_owned(), |tier| tier.to_string()),
                esc(npc.title.as_deref().unwrap_or_default()),
            )
        })
        .collect();
    let table = if cast.is_empty() {
        "<p class=\"dim\">No NPCs yet. Add the first below.</p>".to_owned()
    } else {
        format!("<div class=\"scroll\"><table><thead><tr><th></th><th>NPC</th><th>Prefix</th><th>Race</th><th class=\"num\">Tier</th><th>Title</th></tr></thead><tbody>{rows}</tbody></table></div>")
    };
    let body = format!(
        "<header><h1>NPCs</h1><p class=\"dim\">Characters no player owns, shared by staff.</p></header>\
         <section class=\"card\">{table}</section>\
         <section class=\"card\"><h2>Add an NPC</h2><form method=\"post\" action=\"/a/{t}/npcs\" class=\"row\">\
         <label>Name<input name=\"name\" maxlength=\"{NAME_MAX}\" required></label>\
         <label>Prefix, for posting as them<input name=\"prefix\" maxlength=\"12\" placeholder=\"yen\" autocomplete=\"off\"></label>\
         <button type=\"submit\">Add NPC</button></form></section>"
    );
    let told = web.told(&session);
    Ok(Html(html::page(&session.token, Section::Npcs, "NPCs", told.as_ref(), &body)).into_response())
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct NewNpcInput {
    name: String,
    prefix: String,
}

pub(super) async fn add_npc(State(web): State<Web>, Path(token): Path<String>, Form(input): Form<NewNpcInput>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    if !web.logged()? {
        return Ok(web.back(&session, "/npcs", refused(NO_MOD_LOG)));
    }
    let form = SheetForm { name: input.name, prefix: input.prefix, ..SheetForm::default() };
    match web.services.sheets().save(Target::NewNpc, &form, session.staff)? {
        SheetSaved::Done { after, .. } => {
            let prefix = after.prefix.as_deref().map(|p| format!(" (`{p}:`)")).unwrap_or_default();
            web.audit(&format!("🎭 <@{}> added NPC **{}**{prefix}", session.staff, plain(&after.name)));
            Ok(web.back(&session, &format!("/characters/{}", after.id), done(format!("{} joined the cast.", after.name))))
        }
        other => Ok(web.back(&session, "/npcs", refused(refusal(&other)))),
    }
}

pub(super) async fn show(State(web): State<Web>, Path((token, id)): Path<(String, i64)>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let told = web.told(&session);
    sheet_page(&web, &session, id, None, told).await
}

// The sheet page: as stored, or with the typed values of a refused save.
async fn sheet_page(web: &Web, session: &Session, id: i64, typed: Option<SheetInput>, told: Option<Notice>) -> Page {
    let Some(profile) = web.services.characters().profile(id)? else {
        return Ok(web.back(session, "/characters", refused(GONE)));
    };
    let row = &profile.character;
    let active = web.services.feats().list(id, FeatState::Active)?;
    let archived = web.services.feats().list(id, FeatState::Archived)?;
    let mcguffins = web.services.mcguffins().all()?;

    let mut ids: Vec<u64> = std::iter::once(row.player_id).chain(archived.iter().filter_map(|f| f.archived.as_ref().map(|a| a.by))).collect();
    ids.sort_unstable();
    ids.dedup();
    let names = web.guild.names(ids).await;
    let expressions = web.services.sheets().expressions(id)?;
    let links = row.picture_url.iter().cloned().chain(expressions.iter().map(|e| e.url.clone())).collect();
    let fresh = web.guild.fresh(links).await;
    let link = picture_link(row, &fresh);

    let t = esc(&session.token);
    let values = typed.unwrap_or_else(|| SheetInput::of(row));
    let picture_note = match (row.picture_message, row.picture_url.as_deref()) {
        (Some(_), _) => "Uploaded picture in use. A link typed here replaces it.",
        (None, Some(_)) => "A linked picture. Blank it, or tick the box, for their Discord icon.",
        (None, None) => "None: the card shows their Discord icon.",
    };
    let sheet = format!(
        "<form method=\"post\" action=\"/a/{t}/characters/{id}\" class=\"grid\">\
         <label>Name<input name=\"name\" value=\"{name}\" maxlength=\"{NAME_MAX}\" required></label>\
         <label>Prefix<input name=\"prefix\" value=\"{prefix}\" maxlength=\"12\" autocomplete=\"off\"></label>\
         <label>Race<input name=\"race\" value=\"{race}\" maxlength=\"{NAME_MAX}\"></label>\
         <label>Tier<input name=\"tier\" value=\"{tier}\" inputmode=\"numeric\" maxlength=\"2\"></label>\
         <label>Title<input name=\"title\" value=\"{title}\" maxlength=\"{NAME_MAX}\"></label>\
         <label>Picture link (https)<input name=\"picture_url\" value=\"{picture}\" maxlength=\"400\" placeholder=\"https://…\"></label>\
         <p class=\"dim small wide\">{picture_note}</p>\
         <label class=\"check wide\"><input type=\"checkbox\" name=\"icon\" value=\"{ICON_ON}\"> Use their Discord icon instead</label>\
         <label class=\"check wide\"><input type=\"checkbox\" name=\"posting\" value=\"{ICON_ON}\"{posting}> Posts by prefix: <code>prefix: text</code> posts as them</label>\
         <label class=\"wide\">Bio, shown on their card<textarea name=\"bio\" maxlength=\"1000\">{bio}</textarea></label>\
         <label class=\"wide\">Staff notes, never shown to players<textarea name=\"notes\" maxlength=\"1000\">{notes}</textarea></label>\
         <div><button type=\"submit\">Save sheet</button></div></form>",
        name = esc(&values.name),
        prefix = esc(&values.prefix),
        race = esc(&values.race),
        tier = esc(&values.tier),
        title = esc(&values.title),
        picture = esc(&values.picture_url),
        bio = esc(&values.bio),
        notes = esc(&values.notes),
        posting = if values.posting == ICON_ON { " checked" } else { "" },
    );
    let ticket = format!("/a/{t}/characters/{id}/upload-ticket");
    let upload = picture_form(&ticket);

    let carries = carries(&t, &profile, &mcguffins);
    let expressions = expressions_card(&format!("/a/{t}/characters/{id}/expressions"), &ticket, row, &expressions, &fresh);
    let feats = feats_card(&t, web.guild.server(), id, &active, &archived, &names);
    let subtitle = [row.race.as_deref().map(esc), row.tier.map(|tier| format!("Tier {tier}")), row.title.as_deref().map(|title| format!("<i>{}</i>", esc(title)))]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    let back = if row.player_id == CAST { format!("<a href=\"/a/{t}/npcs\">← NPCs</a>") } else { format!("<a href=\"/a/{t}/players/{}\">← Their player</a>", row.player_id) };
    let body = format!(
        "<header class=\"head\">{portrait}<div><p>{back}</p><h1>{name}</h1><p>{subtitle}</p><p class=\"dim\">{whose} · slot {slot} · {posts} posts</p></div></header>\
         <div class=\"cols\"><section class=\"card\"><h2>Sheet</h2>{sheet}</section>\
         <div style=\"display:grid;gap:18px\"><section class=\"card\"><h2>Picture</h2>{upload}</section>{expressions}{carries}</div></div>{feats}{UPLOAD_SCRIPT}",
        portrait = portrait(link.as_deref(), &row.name),
        name = esc(&row.name),
        whose = whose(row, &names),
        slot = row.slot,
        posts = row.posts,
    );
    let section = if row.player_id == CAST { Section::Npcs } else { Section::Characters };
    Ok(Html(html::page(&session.token, section, &row.name, told.as_ref(), &body)).into_response())
}

// What the character carries, and the McGuffins staff may give or take back here.
fn carries(t: &str, profile: &crate::services::characters::Profile, mcguffins: &[crate::services::mcguffins::McguffinView]) -> String {
    let id = profile.character.id;
    let balls: Vec<_> = profile.balls.iter().map(|(set, star)| esc(&ball_label(*set, *star))).collect();
    let items: Vec<_> = profile.items.iter().map(|(item, n)| if *n > 1 { format!("{} ×{n}", esc(item.label())) } else { esc(item.label()) }).collect();
    let listed = |list: Vec<String>| if list.is_empty() { "<span class=\"dim\">none</span>".to_owned() } else { list.join(", ") };

    let held: String = mcguffins
        .iter()
        .filter(|m| m.holders.iter().any(|(holder, _, _)| *holder == id))
        .map(|m| {
            format!(
                "<li class=\"inline\">{} <a href=\"/a/{t}/mcguffins/{}\">{}</a><form method=\"post\" action=\"/a/{t}/mcguffins/{}/take\">\
                 <input type=\"hidden\" name=\"character\" value=\"{id}\"><input type=\"hidden\" name=\"back\" value=\"{BACK_CHARACTER}\">\
                 <button class=\"quiet\" type=\"submit\">Take back</button></form></li>",
                esc(&m.emoji),
                m.id,
                esc(&m.name),
                m.id
            )
        })
        .collect();
    let givable: Vec<(String, String)> = mcguffins
        .iter()
        .filter(|m| !m.retired && !m.holders.iter().any(|(holder, _, _)| *holder == id))
        .map(|m| (m.id.to_string(), format!("{} {}", m.emoji, m.name)))
        .collect();
    let give = if givable.is_empty() {
        String::new()
    } else {
        format!(
            "<form method=\"post\" action=\"/a/{t}/characters/{id}/mcguffins\" class=\"row\">\
             <label>Give a McGuffin<select name=\"mcguffin\">{}</select></label><button type=\"submit\">Give</button></form>",
            options(&givable, "")
        )
    };
    let held = if held.is_empty() { "<p class=\"dim\">No McGuffins.</p>".to_owned() } else { format!("<ul style=\"margin:0;padding-left:18px\">{held}</ul>") };
    format!(
        "<section class=\"card\"><h2>Carries</h2><p><b>Dragon Balls</b> {}</p><p><b>Items</b> {}</p><h2>McGuffins</h2>{held}{give}</section>",
        listed(balls),
        listed(items)
    )
}

/// A character's expressions: each with its picture and colour, a remove button, and a form to add one.
/// `action` is where the forms post: the admin sheet's, or the player's own page's; `ticket` is where an
/// upload asks for its ticket.
pub(super) fn expressions_card(action: &str, ticket: &str, row: &CharacterRow, list: &[ExpressionRow], fresh: &HashMap<String, String>) -> String {
    let upload = expression_form(ticket);
    let action = esc(action);
    let prefix = row.prefix.as_deref().unwrap_or("prefix");
    let rows: String = list
        .iter()
        .map(|e| {
            let color = e.color.map(|c| format!(" <span class=\"tag\">#{c:06X}</span>")).unwrap_or_default();
            format!(
                "<li class=\"inline\"><img class=\"pic\" src=\"{}\" alt=\"\"> <code>{}:{}</code>{color}<form method=\"post\" action=\"{action}/remove\">\
                 <input type=\"hidden\" name=\"name\" value=\"{}\"><button class=\"quiet\" type=\"submit\">Remove</button></form></li>",
                esc(&fresh_or(&e.url, fresh)),
                esc(prefix),
                esc(&e.name),
                esc(&e.name)
            )
        })
        .collect();
    let list = if rows.is_empty() { "<p class=\"dim\">None yet.</p>".to_owned() } else { format!("<ul style=\"margin:0;padding-left:0;list-style:none;display:grid;gap:6px\">{rows}</ul>") };
    format!(
        "<section class=\"card\"><h2>Expressions</h2><p class=\"dim small\">Other pictures to post with: <code>{p}:happy text</code>. An unknown one posts with the main picture.</p>{list}\
         <form method=\"post\" action=\"{action}\" class=\"grid\"><label>Name<input name=\"name\" maxlength=\"20\" placeholder=\"happy\" required></label>\
         <label>Image link (https)<input name=\"url\" maxlength=\"400\" placeholder=\"https://…\" required></label>\
         <label>Colour bar, optional<input name=\"color\" maxlength=\"7\" placeholder=\"#C0392B\"></label>\
         <div><button type=\"submit\">Save expression</button></div></form>\
         <p class=\"dim small\">Or upload the image:</p>{upload}</section>",
        p = esc(prefix)
    )
}

/// Why an expression was refused, in a sentence.
pub(super) fn expression_refusal(outcome: &ExpressionSaved) -> &'static str {
    match outcome {
        ExpressionSaved::Done(_) => "",
        ExpressionSaved::NotFound => GONE,
        ExpressionSaved::BadName => "Name: 1 to 20 letters, digits, - or _.",
        ExpressionSaved::BadLink => "Image: an https:// link.",
        ExpressionSaved::BadColor => "Colour: six hex digits, like #C0392B, or blank.",
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct ExpressionInput {
    pub(super) name: String,
    pub(super) url: String,
    pub(super) color: String,
}

pub(super) async fn add_expression(State(web): State<Web>, Path((token, id)): Path<(String, i64)>, Form(input): Form<ExpressionInput>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let path = format!("/characters/{id}");
    if !web.logged()? {
        return Ok(web.back(&session, &path, refused(NO_MOD_LOG)));
    }
    let Some(row) = web.services.sheets().get(id)? else {
        return Ok(web.back(&session, "/characters", refused(GONE)));
    };
    let notice = match web.services.sheets().save_expression(id, &input.name, &input.url, &input.color)? {
        ExpressionSaved::Done(e) => {
            web.audit(&format!("🎭 <@{}> saved expression **{}** for **{}** ({})", session.staff, plain(&e.name), plain(&row.name), owner(row.player_id)));
            done(format!("Saved {}'s {} expression.", row.name, e.name))
        }
        other => refused(expression_refusal(&other)),
    };
    Ok(web.back(&session, &path, notice))
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct NameInput {
    pub(super) name: String,
}

pub(super) async fn remove_expression(State(web): State<Web>, Path((token, id)): Path<(String, i64)>, Form(input): Form<NameInput>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let path = format!("/characters/{id}");
    if !web.logged()? {
        return Ok(web.back(&session, &path, refused(NO_MOD_LOG)));
    }
    let Some(row) = web.services.sheets().get(id)? else {
        return Ok(web.back(&session, "/characters", refused(GONE)));
    };
    if !web.services.sheets().remove_expression(id, &input.name)? {
        return Ok(web.back(&session, &path, refused("No such expression.")));
    }
    web.audit(&format!("🎭 <@{}> removed expression **{}** from **{}** ({})", session.staff, plain(input.name.trim()), plain(&row.name), owner(row.player_id)));
    Ok(web.back(&session, &path, done("Removed.")))
}

fn post_links(server: u64, feat: &FeatView) -> String {
    let links: Vec<_> = (1..)
        .zip(&feat.links)
        .map(|(n, l)| format!("<a href=\"https://discord.com/channels/{server}/{}/{}\" rel=\"noreferrer\">post {n}</a>", l.channel_id, l.message_id))
        .collect();
    if links.is_empty() { "<span class=\"dim\">—</span>".to_owned() } else { links.join(" ") }
}

fn feats_card(t: &str, server: u64, id: i64, active: &[FeatView], archived: &[FeatView], names: &HashMap<u64, String>) -> String {
    let active_rows: String = active
        .iter()
        .map(|f| {
            format!(
                "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td><form method=\"post\" action=\"/a/{t}/feats/{}/archive\" class=\"inline\">\
                 <input name=\"reason\" maxlength=\"{REASON_MAX}\" placeholder=\"why archive\" style=\"width:9em\"><button class=\"quiet\" type=\"submit\">Archive</button></form></td></tr>",
                esc(&f.code),
                esc(f.kind.label()),
                esc(f.counts.label()),
                esc(&f.note),
                post_links(server, f),
                f.id
            )
        })
        .collect();
    let archived_rows: String = archived
        .iter()
        .map(|f| {
            let mark = f.archived.as_ref();
            let why = mark.and_then(|m| m.reason.as_deref()).map(esc).unwrap_or_default();
            let by = mark.map(|m| format!("{} · {}", person(m.by, names), html::day(m.at))).unwrap_or_default();
            format!(
                "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{why}</td><td class=\"small\">{by}</td><td><form method=\"post\" action=\"/a/{t}/feats/{}/restore\">\
                 <button class=\"quiet\" type=\"submit\">Restore</button></form></td></tr>",
                esc(&f.code),
                esc(f.kind.label()),
                esc(&f.note),
                f.id
            )
        })
        .collect();

    let kinds: Vec<_> = FeatKind::ALL.iter().map(|k| (k.key().to_owned(), k.label().to_owned())).collect();
    let counts: Vec<_> = FeatCounts::ALL.iter().map(|c| (c.key().to_owned(), format!("Counts as: {}", c.label()))).collect();
    let add = format!(
        "<form method=\"post\" action=\"/a/{t}/characters/{id}/feats\" class=\"grid\">\
         <label>Kind<select name=\"kind\">{}</select></label><label>In a tier-up<select name=\"counts\">{}</select></label>\
         <label class=\"wide\">Post link, optional<input name=\"link\" placeholder=\"https://discord.com/channels/…\"></label>\
         <label class=\"wide\">What staff saw<textarea name=\"note\" maxlength=\"{NOTE_MAX}\" required></textarea></label>\
         <div><button type=\"submit\">Log feat</button></div></form>",
        options(&kinds, ""),
        options(&counts, "")
    );
    let active_table = if active.is_empty() {
        "<p class=\"dim\">No active feats.</p>".to_owned()
    } else {
        format!("<div class=\"scroll\"><table><thead><tr><th>Feat</th><th>Kind</th><th>Counts</th><th>Note</th><th>Posts</th><th></th></tr></thead><tbody>{active_rows}</tbody></table></div>")
    };
    let archived_table = if archived.is_empty() {
        String::new()
    } else {
        format!(
            "<h2>Archived</h2><div class=\"scroll\"><table><thead><tr><th>Feat</th><th>Kind</th><th>Note</th><th>Why</th><th>By</th><th></th></tr></thead><tbody>{archived_rows}</tbody></table></div>"
        )
    };
    format!(
        "<section class=\"card\"><h2>🔒 Feats</h2><p class=\"dim small\">Silent: players never see these.</p>{active_table}{archived_table}\
         <h2>Log a feat</h2>{add}</section>"
    )
}

pub(super) async fn save(State(web): State<Web>, Path((token, id)): Path<(String, i64)>, Form(input): Form<SheetInput>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    if !web.logged()? {
        return sheet_page(&web, &session, id, Some(input), Some(refused(NO_MOD_LOG))).await;
    }

    let outcome = web.services.sheets().save(Target::Existing(id), &input.form(), session.staff)?;
    let (before, after) = match outcome {
        SheetSaved::Done { before, after } => (before, after),
        SheetSaved::NotFound => return Ok(web.back(&session, "/characters", refused(GONE))),
        other => return sheet_page(&web, &session, id, Some(input), Some(refused(refusal(&other)))).await,
    };
    let fields = before.map(|b| changed(&b, &after)).unwrap_or_default();
    if fields.is_empty() {
        return Ok(web.back(&session, &format!("/characters/{id}"), done("Saved. Nothing had changed.")));
    }
    let what = if after.player_id == CAST { "🎭" } else { "👤" };
    web.audit(&format!("{what} <@{}> edited **{}**'s sheet ({}): {}", session.staff, plain(&after.name), owner(after.player_id), fields.join(", ")));
    Ok(web.back(&session, &format!("/characters/{id}"), done(format!("Saved: {}.", fields.join(", ")))))
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct GiveInput {
    mcguffin: String,
}

pub(super) async fn give_mcguffin(State(web): State<Web>, Path((token, id)): Path<(String, i64)>, Form(input): Form<GiveInput>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let mcguffin = input.mcguffin.trim().parse::<i64>().unwrap_or_default();
    give_to(&web, &session, mcguffin, id, Back::Character)
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct NewFeatInput {
    kind: String,
    counts: String,
    note: String,
    link: String,
}

pub(super) async fn add_feat(State(web): State<Web>, Path((token, id)): Path<(String, i64)>, Form(input): Form<NewFeatInput>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let path = format!("/characters/{id}");
    if !web.logged()? {
        return Ok(web.back(&session, &path, refused(NO_MOD_LOG)));
    }
    let (Some(kind), Some(counts)) = (FeatKind::from_key(&input.kind), FeatCounts::from_key(&input.counts)) else {
        return Ok(web.back(&session, &path, refused("Pick a kind and what it counts as.")));
    };
    let post = match input.link.trim() {
        "" => None,
        typed => match parse_link(typed) {
            Some(link) if link.guild_id == web.guild.server() => Some(PostRef { channel_id: link.channel_id, message_id: link.message_id }),
            Some(_) => return Ok(web.back(&session, &path, refused("That post is on another server."))),
            None => return Ok(web.back(&session, &path, refused("That is not a message link. Right-click the post › Copy Message Link."))),
        },
    };

    let staff = session.staff;
    let new = NewFeat { character_id: id, kind, counts, note: &input.note, post, staff_id: staff };
    let notice = match web.services.feats().add(&new)? {
        Added::Done(feat) => {
            web.audit(&format!(
                "🔒 <@{staff}> logged {} **{}** · counts {} · {} ({}) · {}",
                feat.kind.label(),
                feat.code,
                feat.counts.label(),
                plain(&feat.character_name),
                owner(feat.player_id),
                plain(&feat.note)
            ));
            done(format!("{} logged.", feat.code))
        }
        Added::NoNote => refused("Write a note: what happened."),
        Added::NoCharacter => refused(GONE),
    };
    Ok(web.back(&session, &path, notice))
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct ArchiveInput {
    reason: String,
}

pub(super) async fn archive(State(web): State<Web>, Path((token, feat)): Path<(String, i64)>, Form(input): Form<ArchiveInput>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let Some(found) = web.services.feats().get(feat)? else {
        return Ok(web.back(&session, "/characters", refused("No such feat.")));
    };
    let path = format!("/characters/{}", found.character_id);
    if !web.logged()? {
        return Ok(web.back(&session, &path, refused(NO_MOD_LOG)));
    }
    let reason = Some(input.reason.trim()).filter(|r| !r.is_empty());
    let notice = match web.services.feats().archive(feat, reason, session.staff)? {
        Moved::Done(f) => {
            let why = reason.map(|r| format!(" · {}", plain(r))).unwrap_or_default();
            web.audit(&format!("🔒 <@{}> archived **{}** ({}){why}", session.staff, f.code, plain(&f.character_name)));
            done(format!("{} archived. Restore brings it back.", f.code))
        }
        Moved::Already => refused("That feat is archived already."),
        Moved::NotFound => refused("No such feat."),
    };
    Ok(web.back(&session, &path, notice))
}

pub(super) async fn restore(State(web): State<Web>, Path((token, feat)): Path<(String, i64)>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let Some(found) = web.services.feats().get(feat)? else {
        return Ok(web.back(&session, "/characters", refused("No such feat.")));
    };
    let path = format!("/characters/{}", found.character_id);
    if !web.logged()? {
        return Ok(web.back(&session, &path, refused(NO_MOD_LOG)));
    }
    let notice = match web.services.feats().restore(feat, session.staff)? {
        Moved::Done(f) => {
            web.audit(&format!("🔒 <@{}> restored **{}** ({})", session.staff, f.code, plain(&f.character_name)));
            done(format!("{} restored.", f.code))
        }
        Moved::Already => refused("That feat is active already."),
        Moved::NotFound => refused("No such feat."),
    };
    Ok(web.back(&session, &path, notice))
}
