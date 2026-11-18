//! The admin pages' frame and parts: the layout, the style, escaping, and small pieces each page shares.
//! Every value from the database, Discord or a form is escaped on the way out.
//!
//! ```text
//!   ┌ 🐉 Raging Spirit · admin   Home  Players  Characters  NPCs  McGuffins  Dragon Balls ┐
//!   │ <notice: what the last form did, once>                                              │
//!   │ <the page>                                                                          │
//!   └ This link is yours alone. It stops after 30 idle minutes. ──────────────────────────┘
//! ```

use std::collections::HashMap;

use crate::domain::dragon_balls::STARS;
use crate::services::balls::SetView;
use crate::services::links::Notice;

const STYLE: &str = "
:root{color-scheme:light;--bg:#f6f4f0;--panel:#fff;--ink:#1d1a16;--muted:#6b645b;--line:#e5dfd6;--accent:#d9541a;--on-accent:#fff;--soft:#fbede3;--ok:#2f7d4f;--bad:#b4332a}
@media (prefers-color-scheme:dark){:root{color-scheme:dark;--bg:#121110;--panel:#1b1917;--ink:#ece7df;--muted:#a39b8f;--line:#2f2b26;--accent:#ff8a3d;--on-accent:#1b1009;--soft:#2b1e14;--ok:#6fcf97;--bad:#ff8a80}}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--ink);font:15px/1.5 system-ui,-apple-system,'Segoe UI',sans-serif}
.top{background:var(--panel);border-bottom:1px solid var(--line);padding:10px max(16px,4vw);display:flex;gap:6px 18px;align-items:center;flex-wrap:wrap}
.brand{font-weight:800;text-decoration:none;color:var(--ink);white-space:nowrap}.brand span{color:var(--accent)}
nav{display:flex;gap:4px;flex-wrap:wrap}
nav a{padding:5px 11px;border-radius:999px;text-decoration:none;color:var(--muted);font-weight:600;font-size:14px}
nav a:hover{background:var(--soft);color:var(--ink)}nav a[aria-current]{background:var(--accent);color:var(--on-accent)}
main{max-width:1100px;margin:0 auto;padding:24px max(16px,4vw) 40px;display:grid;gap:18px}
h1{font-size:26px;margin:0;line-height:1.2}h2{font-size:17px;margin:0}p{margin:0}
a{color:var(--accent)}.dim{color:var(--muted)}.small{font-size:13px}code{font-size:13px}
.card{background:var(--panel);border:1px solid var(--line);border-radius:12px;padding:16px;display:grid;gap:12px;min-width:0;align-content:start}
.cols{display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,340px),1fr));gap:18px;align-items:start}
.tiles{display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,160px),1fr));gap:12px}
.tile{background:var(--panel);border:1px solid var(--line);border-radius:12px;padding:14px 16px;text-decoration:none;color:var(--ink)}
.tile:hover{border-color:var(--accent)}.tile b{display:block;font-size:28px;line-height:1.1;color:var(--accent)}
.scroll{overflow-x:auto}
table{width:100%;border-collapse:collapse}th,td{text-align:left;padding:8px;border-bottom:1px solid var(--line);vertical-align:middle}
th{font-size:12px;text-transform:uppercase;letter-spacing:.05em;color:var(--muted)}tbody tr:last-child td{border-bottom:0}
td.num,th.num{text-align:right;font-variant-numeric:tabular-nums}
.head{display:flex;gap:16px;align-items:center;flex-wrap:wrap}
.portrait{width:96px;height:96px;border-radius:16px;object-fit:cover;background:var(--soft);display:grid;place-items:center;font-size:40px;font-weight:800;color:var(--accent);flex:none}
.pic{width:32px;height:32px;border-radius:50%;object-fit:cover;vertical-align:middle;background:var(--soft)}
form{margin:0}form.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,200px),1fr));gap:12px}
label{display:grid;gap:4px;font-size:13px;font-weight:600;color:var(--muted)}.wide{grid-column:1/-1}
label.check{display:flex;gap:8px;align-items:center;font-weight:500;color:var(--ink)}
input,textarea,select{font:inherit;color:var(--ink);background:var(--bg);border:1px solid var(--line);border-radius:8px;padding:7px 9px;width:100%;min-width:0}
input[type=checkbox]{width:auto}input[type=file]{padding:6px}textarea{min-height:96px;resize:vertical}
button{font:600 14px system-ui,sans-serif;background:var(--accent);color:var(--on-accent);border:0;border-radius:8px;padding:8px 14px;cursor:pointer;white-space:nowrap}
button.quiet{background:transparent;color:var(--accent);border:1px solid var(--line);padding:5px 10px}
:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
.notice{border-radius:10px;padding:10px 12px;border:1px solid currentColor}.ok{color:var(--ok)}.bad{color:var(--bad)}
.row{display:flex;gap:10px;align-items:end;flex-wrap:wrap}.row label{flex:1;min-width:min(100%,160px)}
.inline{display:flex;gap:6px;align-items:center;flex-wrap:wrap}
.tag{display:inline-block;font-size:12px;font-weight:700;padding:1px 8px;border-radius:999px;background:var(--soft);color:var(--accent)}
.tree{display:grid;gap:10px}.tree fieldset{border:1px solid var(--line);border-radius:10px;padding:8px 12px;margin:0;display:grid;gap:4px}
.tree legend{padding:0 4px}
footer{max-width:1100px;margin:0 auto;padding:0 max(16px,4vw) 32px;color:var(--muted);font-size:13px}
.cards{display:grid;grid-template-columns:repeat(auto-fill,minmax(min(100%,320px),1fr));gap:18px;align-items:start}
.card.playing{border-color:var(--accent);box-shadow:0 0 0 1px var(--accent)}
.bio{white-space:pre-line}
details{border-top:1px solid var(--line);padding-top:8px}summary{cursor:pointer;color:var(--muted)}details .card{border:0;padding:8px 0 0}
";

/// Where a page sits in the menu.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Section {
    Home,
    Players,
    Characters,
    Npcs,
    Mcguffins,
    Balls,
    Worlds,
}

const SECTIONS: [(Section, &str, &str); 7] = [
    (Section::Home, "", "Home"),
    (Section::Players, "/players", "Players"),
    (Section::Characters, "/characters", "Characters"),
    (Section::Npcs, "/npcs", "NPCs"),
    (Section::Mcguffins, "/mcguffins", "McGuffins"),
    (Section::Balls, "/balls", "Dragon Balls"),
    (Section::Worlds, "/worlds", "Worlds"),
];

/// HTML-escaped: safe inside text and quoted attributes.
pub(super) fn esc(text: &str) -> String {
    text.chars().fold(String::with_capacity(text.len()), |mut out, c| {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
        out
    })
}

/// Discord markdown shown as typed, for a mod log line.
pub(super) fn plain(text: &str) -> String {
    text.chars().fold(String::with_capacity(text.len()), |mut out, c| {
        if matches!(c, '*' | '_' | '~' | '`' | '|' | '>' | '#' | '[' | ']' | '<' | '@') {
            out.push('\\');
        }
        out.push(c);
        out
    })
}

/// A value or a dash.
pub(super) fn or_dash(text: Option<&str>) -> String {
    text.map_or_else(|| "—".to_owned(), esc)
}

/// `<option>`s, the one matching `selected` picked.
pub(super) fn options(choices: &[(String, String)], selected: &str) -> String {
    choices
        .iter()
        .map(|(value, label)| {
            let picked = if value == selected { " selected" } else { "" };
            format!("<option value=\"{}\"{picked}>{}</option>", esc(value), esc(label))
        })
        .collect()
}

/// Someone by their server name, else their id.
pub(super) fn person(id: u64, names: &HashMap<u64, String>) -> String {
    match names.get(&id) {
        Some(name) => format!("{} <span class=\"dim small\">{id}</span>", esc(name)),
        None => format!("<span class=\"dim\">{id}</span>"),
    }
}

/// A big picture, or the name's first letter when there is none.
pub(super) fn portrait(link: Option<&str>, name: &str) -> String {
    match link.filter(|url| url.starts_with("https://")) {
        Some(url) => format!("<img class=\"portrait\" src=\"{}\" alt=\"\">", esc(url)),
        None => format!("<div class=\"portrait\" aria-hidden=\"true\">{}</div>", esc(&name.chars().next().unwrap_or('?').to_string())),
    }
}

/// A day from milliseconds since the epoch: "2026-11-17", in UTC.
pub(super) fn day(ms: i64) -> String {
    const MS_PER_DAY: i64 = 86_400_000;
    // Howard Hinnant's days-to-civil: proleptic Gregorian, 400-year eras of 146 097 days.
    let z = ms.div_euclid(MS_PER_DAY) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn notice(told: Option<&Notice>) -> String {
    match told {
        None => String::new(),
        Some(Notice::Done(text)) => format!("<p class=\"notice ok\" role=\"status\">{}</p>", esc(text)),
        Some(Notice::Refused(text)) => format!("<p class=\"notice bad\" role=\"alert\">{}</p>", esc(text)),
    }
}

fn shell(title: &str, top: &str, body: &str, foot: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <meta name=\"referrer\" content=\"no-referrer\"><meta name=\"robots\" content=\"noindex\"><title>{} · Raging Spirit</title>\
         <style>{STYLE}</style></head><body>{top}<main>{body}</main>{foot}</body></html>",
        esc(title)
    )
}

// The bar on top: the brand, then the menu with the current page marked.
fn bar<S: PartialEq>(home: &str, brand: &str, items: &[(S, &str, &str)], current: &S) -> String {
    let links: String = items
        .iter()
        .map(|(s, path, label)| {
            let mark = if s == current { " aria-current=\"page\"" } else { "" };
            format!("<a href=\"{home}{path}\"{mark}>{label}</a>")
        })
        .collect();
    format!("<header class=\"top\"><a class=\"brand\" href=\"{home}\">🐉 Raging Spirit{brand}</a><nav>{links}</nav></header>")
}

fn foot(command: &str) -> String {
    format!("<footer>This link is yours alone. It stops after 30 minutes without use, 4 hours at most: ask for a new one with {command}.</footer>")
}

/// An admin page: the menu, what the last form did, the page, the footer.
pub(super) fn page(token: &str, section: Section, title: &str, told: Option<&Notice>, body: &str) -> String {
    let top = bar(&format!("/a/{}", esc(token)), " <span>admin</span>", &SECTIONS, &section);
    shell(title, &top, &format!("{}{body}", notice(told)), &foot("/z_moderator_admin"))
}

/// Where a player's page sits in their menu.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mine {
    Me,
    Players,
}

const MINE: [(Mine, &str, &str); 2] = [(Mine::Me, "", "My characters"), (Mine::Players, "/players", "Players")];

/// A player's page: their menu, what the last form did, the page, the footer.
pub(super) fn player_page(token: &str, section: Mine, title: &str, told: Option<&Notice>, body: &str) -> String {
    let top = bar(&format!("/p/{}", esc(token)), "", &MINE, &section);
    shell(title, &top, &format!("{}{body}", notice(told)), &foot("/mypage"))
}

fn bare(title: &str, body: &str) -> String {
    let top = "<header class=\"top\"><span class=\"brand\">🐉 Raging Spirit</span></header>";
    shell(title, top, &format!("<section class=\"card\">{body}</section>"), "")
}

pub(super) fn expired() -> String {
    bare(
        "Link expired",
        "<h1>This link has expired</h1><p>Ask for a new one in Discord: <code>/mypage</code>, or <code>/z_moderator_admin</code> for staff. It comes by DM.</p>",
    )
}

pub(super) fn failed() -> String {
    bare("Something failed", "<h1>Something failed</h1><p>It is in the bot's log. Go back and try again, or tell whoever runs the bot.</p>")
}

/// The mod log as the home page shows it.
pub(super) enum ModLog {
    Set(String),
    Unset,
}

pub(super) struct Counts<'a> {
    pub(super) players: usize,
    pub(super) characters: usize,
    pub(super) cast: usize,
    pub(super) mcguffins: usize,
    pub(super) worlds: usize,
    pub(super) sets: &'a [SetView],
}

pub(super) fn home(token: &str, told: Option<&Notice>, counts: &Counts, mod_log: &ModLog) -> String {
    let t = esc(token);
    let tile = |path: &str, n: usize, label: &str| format!("<a class=\"tile\" href=\"/a/{t}{path}\"><b>{n}</b>{label}</a>");
    let tiles = [
        tile("/players", counts.players, "players"),
        tile("/characters", counts.characters, "characters"),
        tile("/npcs", counts.cast, "NPCs"),
        tile("/mcguffins", counts.mcguffins, "McGuffins in play"),
        tile("/worlds", counts.worlds, "worlds"),
    ]
    .concat();

    let sets: String = counts
        .sets
        .iter()
        .map(|view| {
            let held = view.balls.iter().filter(|b| b.holder.is_some()).count();
            let hidden = view.balls.iter().filter(|b| b.holder.is_none() && b.channel_id.is_some()).count();
            format!(
                "<tr><td><a href=\"/a/{t}/balls/{}\">{}</a></td><td class=\"num\">{held}</td><td class=\"num\">{hidden}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
                view.set.key(),
                esc(view.set.label()),
                STARS.len() - held - hidden,
                view.need
            )
        })
        .collect();

    let log = match mod_log {
        ModLog::Set(name) => format!("<p>Every change is posted to <b>#{}</b>.</p>", esc(name)),
        ModLog::Unset => format!("<p class=\"notice bad\">{}</p>", esc(super::NO_MOD_LOG)),
    };
    let body = format!(
        "<header><h1>Raging Spirit</h1><p class=\"dim\">Players, characters, NPCs, McGuffins and Dragon Balls, for staff.</p></header>\
         <div class=\"tiles\">{tiles}</div>\
         <div class=\"cols\"><section class=\"card\"><h2>Dragon Balls</h2><div class=\"scroll\"><table><thead><tr><th>Set</th><th class=\"num\">Held</th>\
         <th class=\"num\">Hidden</th><th class=\"num\">Not placed</th><th class=\"num\">Roll</th></tr></thead><tbody>{sets}</tbody></table></div></section>\
         <section class=\"card\"><h2>Mod log</h2>{log}<p class=\"dim small\">Changes made here show in the mod log as “· admin pages”.</p></section></div>"
    );
    page(token, Section::Home, "Home", told, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_escaped_and_only_https_pictures_load() {
        assert_eq!(esc("<b a=\"1\">'&'</b>"), "&lt;b a=&quot;1&quot;&gt;&#39;&amp;&#39;&lt;/b&gt;");
        assert!(!portrait(Some("javascript:alert(1)"), "Yenma").contains("src="));
        assert!(portrait(Some("https://cdn.discordapp.com/a.png"), "Yenma").contains("src=\"https://cdn.discordapp.com/a.png\""));
        assert!(portrait(None, "<Yenma>").contains("&lt;"));
    }

    #[test]
    fn mod_log_text_shows_markdown_and_mentions_as_typed() {
        assert_eq!(plain("**Goku** <@1> @everyone"), "\\*\\*Goku\\*\\* \\<\\@1\\> \\@everyone");
    }

    #[test]
    fn days_are_civil_dates_in_utc() {
        assert_eq!(day(0), "1970-01-01");
        assert_eq!(day(1_795_392_000_000), "2026-11-23");
        assert_eq!(day(951_782_400_000), "2000-02-29");
    }

    #[test]
    fn the_menu_marks_the_page_it_is_on() {
        let html = page("abc", Section::Npcs, "NPCs", None, "");
        assert!(html.contains("<a href=\"/a/abc/npcs\" aria-current=\"page\">NPCs</a>"));
        assert!(html.contains("<a href=\"/a/abc/players\">Players</a>"));
    }
}
