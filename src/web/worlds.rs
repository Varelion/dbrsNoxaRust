//! Worlds: which of the server's categories are worlds. A world's text channels are the roleplay
//! channels, where a McGuffin placed at random may land.
//!
//! ```text
//!   ☑ Earth        4 text channels          the server's categories, read when the page opens
//!   ☐ Out of character
//!   [Save worlds]  ─► + <#Earth> − <#OOC> in the mod log
//! ```

use std::collections::HashMap;

use axum::extract::{Form, Path, State};
use axum::response::{Html, IntoResponse};

use super::html::{self, Section, esc};
use super::{NO_MOD_LOG, Page, Web, done, expired, refused};
use crate::services::guild::ChannelKind;
use crate::services::worlds::roleplay;

const WORLD_FIELD: &str = "world_";

pub(super) async fn show(State(web): State<Web>, Path(token): Path<String>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let channels = web.guild.channels().await;
    let worlds = web.services.worlds().all()?;
    let t = esc(&session.token);

    let rows: String = channels
        .iter()
        .filter(|c| c.kind == ChannelKind::Category)
        .map(|category| {
            let texts = channels.iter().filter(|c| c.kind == ChannelKind::Text && c.parent == Some(category.id)).count();
            let on = if worlds.contains(&category.id) { " checked" } else { "" };
            format!(
                "<tr><td><label class=\"check\"><input type=\"checkbox\" name=\"{WORLD_FIELD}{}\" value=\"on\"{on}> <b>{}</b></label></td><td class=\"num\">{texts}</td></tr>",
                category.id,
                esc(&category.name)
            )
        })
        .collect();
    let gone = worlds.iter().filter(|w| !channels.iter().any(|c| c.id == **w)).count();
    let gone = if gone > 0 && !channels.is_empty() { format!("<p class=\"notice bad\">{gone} world(s) the server no longer has: saving drops them.</p>") } else { String::new() };
    let body = if channels.is_empty() {
        "<p class=\"notice bad\">Discord did not list the server's channels. Reload in a moment.</p>".to_owned()
    } else {
        format!(
            "{gone}<form method=\"post\" action=\"/a/{t}/worlds\"><div class=\"scroll\"><table><thead><tr><th>Category</th><th class=\"num\">Text channels</th></tr></thead>\
             <tbody>{rows}</tbody></table></div><p><button type=\"submit\">Save worlds</button></p></form>"
        )
    };
    let page = format!(
        "<header><h1>Worlds</h1><p class=\"dim\">Tick each category that is a world. Its text channels are roleplay channels: {} now.</p></header>\
         <section class=\"card\">{body}</section>",
        roleplay(&worlds, &channels).len()
    );
    let told = web.told(&session);
    Ok(Html(html::page(&session.token, Section::Worlds, "Worlds", told.as_ref(), &page)).into_response())
}

pub(super) async fn save(State(web): State<Web>, Path(token): Path<String>, Form(input): Form<HashMap<String, String>>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    if !web.logged()? {
        return Ok(web.back(&session, "/worlds", refused(NO_MOD_LOG)));
    }
    // An empty list from Discord would untick every world: nothing is saved without the real one.
    let channels = web.guild.channels().await;
    if channels.is_empty() {
        return Ok(web.back(&session, "/worlds", refused("Discord did not list the server's channels; nothing changed. Try again in a moment.")));
    }
    let mut picked: Vec<u64> = input
        .keys()
        .filter_map(|name| name.strip_prefix(WORLD_FIELD)?.parse::<u64>().ok())
        .filter(|id| channels.iter().any(|c| c.id == *id && c.kind == ChannelKind::Category))
        .collect();
    picked.sort_unstable();

    let before = web.services.worlds().set(&picked)?;
    let added: Vec<_> = picked.iter().filter(|w| !before.contains(w)).map(|w| format!("+ <#{w}>")).collect();
    let removed: Vec<_> = before.iter().filter(|w| !picked.contains(w)).map(|w| format!("− <#{w}>")).collect();
    let changes = [added, removed].concat();
    if changes.is_empty() {
        return Ok(web.back(&session, "/worlds", done("Saved. Nothing had changed.")));
    }
    web.audit(&format!("🌍 <@{}> changed the worlds: {}", session.staff, changes.join(" ")));
    let channels_now = roleplay(&picked, &channels).len();
    Ok(web.back(&session, "/worlds", done(format!("{} worlds, {channels_now} roleplay channels.", picked.len()))))
}
