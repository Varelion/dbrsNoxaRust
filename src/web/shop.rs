//! The shop's shelf, for staff: add, edit, hide and remove items. What each player owns is on their
//! player page; players buy on their own page.
//!
//! ```text
//!   ┌ Shop ───────────────────────────────────────────────┐
//!   │ [emoji] [name] [price] [description] ☑ listed [Add] │
//!   │ 🫘 Senzu Bean   25 SP   listed   3 owners  ▸ Edit   │
//!   │ 🎁 Starter Capsule  Free  hidden  0 owners  ▸ Edit  │
//!   └─────────────────────────────────────────────────────┘
//! ```

use axum::extract::{Form, Path, State};
use axum::response::{Html, IntoResponse};
use serde::Deserialize;

use super::html::{self, Section, esc, plain};
use super::{NO_MOD_LOG, Page, Web, done, expired, refused};
use crate::domain::shop::{DESCRIPTION_MAX, EMOJI_MAX, NAME_MAX, PRICE_MAX, price_label};
use crate::domain::tutorial::Did;
use crate::services::shop::{ItemInput, Listing, Saved, Shelf};
use crate::store::shop::ItemRow;

const PATH: &str = "/shop";

// The fields of an item, empty for a new one.
fn item_fields(item: Option<&ItemRow>) -> String {
    let value = |f: fn(&ItemRow) -> String| item.map(f).map(|v| esc(&v)).unwrap_or_default();
    let listed = item.is_none_or(|i| i.listed);
    format!(
        "<label>Emoji<input name=\"emoji\" value=\"{}\" maxlength=\"{EMOJI_MAX}\" placeholder=\"🫘\" required></label>\
         <label>Name<input name=\"name\" value=\"{}\" maxlength=\"{NAME_MAX}\" placeholder=\"Senzu Bean\" required></label>\
         <label>Price in SP (0: free, one each)<input name=\"price\" type=\"number\" min=\"0\" max=\"{PRICE_MAX}\" value=\"{}\" required></label>\
         <label class=\"wide\">Description<input name=\"description\" value=\"{}\" maxlength=\"{DESCRIPTION_MAX}\"></label>\
         <label class=\"check\"><input type=\"checkbox\" name=\"listed\" value=\"on\"{}> Listed: players see it and may buy it</label>",
        value(|i| i.emoji.clone()),
        value(|i| i.name.clone()),
        value(|i| i.price.to_string()),
        value(|i| i.description.clone()),
        if listed { " checked" } else { "" },
    )
}

pub(super) async fn list(State(web): State<Web>, Path(token): Path<String>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    let shop = web.services.shop();
    let items = shop.items(Shelf::All)?;
    let owners = shop.owners()?;
    let t = esc(&session.token);

    let rows: String = items
        .iter()
        .map(|i| {
            let owned = owners.get(&i.id).copied().unwrap_or_default();
            let listed = if i.listed { "<span class=\"tag\">listed</span>" } else { "<span class=\"dim\">hidden</span>" };
            format!(
                "<tr><td><b>{}</b><p class=\"dim small\">{}</p></td><td class=\"num\">{}</td><td>{listed}</td><td class=\"num\">{owned}</td><td>\
                 <details><summary>Edit</summary><form method=\"post\" action=\"/a/{t}/shop/{id}\" class=\"grid\">{}<div><button type=\"submit\">Save</button></div></form>\
                 <form method=\"post\" action=\"/a/{t}/shop/{id}/remove\"><button type=\"submit\" class=\"quiet\">Remove from the shop and every inventory</button></form></details></td></tr>",
                esc(&i.label()),
                esc(&i.description),
                price_label(i.price),
                item_fields(Some(i)),
                id = i.id,
            )
        })
        .collect();
    let table = if items.is_empty() {
        "<p class=\"dim\">The shop is empty.</p>".to_owned()
    } else {
        format!(
            "<div class=\"scroll\"><table><thead><tr><th>Item</th><th class=\"num\">Price</th><th>Players</th><th class=\"num\">Owners</th><th></th></tr></thead><tbody>{rows}</tbody></table></div>"
        )
    };
    let body = format!(
        "<header><h1>Shop</h1><p class=\"dim\">What players buy with Spirit Points, on their page. Set what one player owns on their player page.</p></header>\
         <section class=\"card\"><h2>Add an item</h2><form method=\"post\" action=\"/a/{t}/shop\" class=\"grid\">{}<div><button type=\"submit\">Add item</button></div></form></section>\
         <section class=\"card\">{table}</section>",
        item_fields(None)
    );
    let told = web.told(&session);
    Ok(Html(html::page(&session.token, Section::Shop, "Shop", told.as_ref(), &body)).into_response())
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct ItemForm {
    emoji: String,
    name: String,
    price: String,
    description: String,
    listed: String,
}

impl ItemForm {
    // None: the price is not a whole number.
    fn input(&self) -> Option<ItemInput<'_>> {
        let price = self.price.trim().parse::<i64>().ok()?;
        let listing = if self.listed == "on" { Listing::Listed } else { Listing::Hidden };
        Some(ItemInput { emoji: &self.emoji, name: &self.name, description: &self.description, price, listing })
    }
}

fn refusal(saved: &Saved) -> String {
    match saved {
        Saved::Done(_) => String::new(),
        Saved::NoEmoji => "Give it an emoji.".to_owned(),
        Saved::NoName => "Give it a name.".to_owned(),
        Saved::BadPrice => format!("Price: a whole number from 0 to {PRICE_MAX}."),
        Saved::NameTaken => "Another item has that name.".to_owned(),
        Saved::Missing => "That item is gone.".to_owned(),
    }
}

pub(super) async fn create(State(web): State<Web>, Path(token): Path<String>, Form(form): Form<ItemForm>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    if !web.logged()? {
        return Ok(web.back(&session, PATH, refused(NO_MOD_LOG)));
    }
    let Some(input) = form.input() else {
        return Ok(web.back(&session, PATH, refused(refusal(&Saved::BadPrice))));
    };
    let notice = match web.services.shop().create(&input)? {
        Saved::Done(item) => {
            web.audit(&format!("🛒 <@{}> added {} to the shop: {}", session.staff, plain(&item.label()), price_label(item.price)));
            web.guild.did(session.staff, Did::ShopChanged);
            done(format!("{} is in the shop.", item.label()))
        }
        other => refused(refusal(&other)),
    };
    Ok(web.back(&session, PATH, notice))
}

pub(super) async fn edit(State(web): State<Web>, Path((token, id)): Path<(String, i64)>, Form(form): Form<ItemForm>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    if !web.logged()? {
        return Ok(web.back(&session, PATH, refused(NO_MOD_LOG)));
    }
    let Some(input) = form.input() else {
        return Ok(web.back(&session, PATH, refused(refusal(&Saved::BadPrice))));
    };
    let notice = match web.services.shop().edit(id, &input)? {
        Saved::Done(item) => {
            let listed = if item.listed { "listed" } else { "hidden" };
            web.audit(&format!("🛒 <@{}> edited {} in the shop: {}, {listed}", session.staff, plain(&item.label()), price_label(item.price)));
            web.guild.did(session.staff, Did::ShopChanged);
            done(format!("Saved {}.", item.label()))
        }
        other => refused(refusal(&other)),
    };
    Ok(web.back(&session, PATH, notice))
}

pub(super) async fn remove(State(web): State<Web>, Path((token, id)): Path<(String, i64)>) -> Page {
    let Some(session) = web.staff(&token) else {
        return Ok(expired());
    };
    if !web.logged()? {
        return Ok(web.back(&session, PATH, refused(NO_MOD_LOG)));
    }
    let notice = match web.services.shop().remove(id)? {
        Some((item, owners)) => {
            web.audit(&format!("🛒 <@{}> removed {} from the shop and {owners} inventories", session.staff, plain(&item.label())));
            web.guild.did(session.staff, Did::ShopChanged);
            done(format!("Removed {}; {owners} players lost it.", item.label()))
        }
        None => refused(refusal(&Saved::Missing)),
    };
    Ok(web.back(&session, PATH, notice))
}
