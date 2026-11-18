//! /help [topic]: a page for every command, read from what is registered with Discord so it never
//! drifts, and guides that walk through one job and name the commands that do it. The pebblehost
//! bots' help (shared/help), in Rust.
//!
//! ```text
//!   /help                          the index: guides, then commands; staff also see staff's
//!   /help start                    a guide: the job, step by step, and its commands
//!   /help /z_moderator_sp          a command: a line per action, its options, examples, see also
//!   /help /z_moderator_sp give     narrowed to one action
//!   /help sp · /help .prune · /help log feat
//!
//!   ┌ /z_moderator_sp — Staff: Spirit Points ─────────────────┐
//!   │ /z_moderator_sp action:give player:<user> amount:<…>    │  synopsis
//!   │ ACTIONS   give — Give: add to one player's Spirit Points │
//!   │ OPTIONS   player:<user> — Whose · give, take, set, history │
//!   │ EXAMPLES · SEE ALSO                                      │
//!   └ [ ] optional · < > your value · | one of ────────────────┘
//! ```
//!
//! Only staff see staff pages: in the index, in a lookup and in the menu.

use anyhow::Result;
use twilight_model::application::command::{Command, CommandOption, CommandOptionChoiceValue, CommandOptionType, CommandOptionValue, CommandType};
use twilight_model::channel::message::Embed;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder, EmbedFooterBuilder};

use super::app::Reading;
use super::commands::{self, CHARACTER, DBRS, FEAT, LOG_FEAT, SP};
use super::ctx::{ACTION, Ctx, Options};
use super::dot::{PRUNE, PRUNE_MAX};
use super::eggs::COOKIE;

const DOT_SET: &str = ".set";

/// The option that names a page.
pub(crate) const TOPIC: &str = "topic";
const ACCENT: u32 = 0xF0_7A_1E;
const NOTATION: &str = "[ ] may be left out · < > your value · a|b one of these";
const SLASH: &str = "/";
const STAFF_PREFIX: &str = "z_moderator_";
const OPTIONAL: char = '?';
const CHOICES_SHOWN: usize = 8;
const MENU_MAX: usize = 25;
const CHOICE_MAX: usize = 100;
// Discord's embed limits (spec).
const TITLE_MAX: usize = 256;
const FIELD_MAX: usize = 1024;
const FIELDS_MAX: usize = 25;
const MESSAGE_EMBEDS_MAX: usize = 6000;
// Room kept for the footer and a "too long" note.
const PAGE_ROOM: usize = MESSAGE_EMBEDS_MAX - 300;

/// Who may read a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Audience {
    Everyone,
    Staff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Guide,
    Command,
}

#[derive(Clone, Debug)]
struct Term {
    term: String,
    text: String,
}

#[derive(Clone, Debug)]
struct Page {
    name: String,
    audience: Audience,
    kind: Kind,
    summary: String,
    synopsis: Vec<String>,
    description: Vec<String>,
    commands: Vec<Term>,
    actions: Vec<Term>,
    options: Vec<Term>,
    examples: Vec<String>,
    see_also: Vec<String>,
}

impl Page {
    fn new(name: String, audience: Audience, kind: Kind, summary: String) -> Self {
        Self {
            name,
            audience,
            kind,
            summary,
            synopsis: Vec::new(),
            description: Vec::new(),
            commands: Vec::new(),
            actions: Vec::new(),
            options: Vec::new(),
            examples: Vec::new(),
            see_also: Vec::new(),
        }
    }
}

/// What each action of a command takes, in order; `?` marks one that may be left out. The handlers
/// read the same names: a test checks every action and option here is one Discord has.
const USAGE: &[(&str, &str, &[&str])] = &[
    (DBRS, "mod_log", &["channel"]),
    (SP, "give", &["player", "amount", "memo"]),
    (SP, "take", &["player", "amount", "memo"]),
    (SP, "set", &["player", "amount", "memo"]),
    (SP, "grant", &["role", "amount", "memo"]),
    (SP, "history", &["player"]),
    (CHARACTER, "add", &["player", "slot", "name"]),
    (FEAT, "add", &["player", "character", "kind", "counts", "note", "?link"]),
    (FEAT, "edit", &["feat", "?kind", "?counts", "?note"]),
    (FEAT, "link", &["feat", "link"]),
    (FEAT, "unlink", &["feat", "link"]),
    (FEAT, "archive", &["feat", "?reason"]),
    (FEAT, "restore", &["feat"]),
    (FEAT, "list", &["player", "character", "?state"]),
];

/// A guide: a job, step by step, and the commands that do it, each with its examples.
struct Guide {
    name: &'static str,
    audience: Audience,
    summary: &'static str,
    steps: &'static [&'static str],
    commands: &'static [(&'static str, &'static [&'static str])],
}

const GUIDES: &[Guide] = &[
    Guide {
        name: "start",
        audience: Audience::Everyone,
        summary: "New here: the first three things to do",
        steps: &[
            "1. `/tutorial`: learn by doing it, one task at a time, in a thread of your own.",
            "2. `/mypage`: your page, by DM: your characters, who you play, posting, Spirit Points.",
            "3. `/panel` or `.set1`: switch who you play. `nova: text` posts as them (`/help posting`).",
            "`/help <command>` explains any command.",
        ],
        commands: &[("/tutorial", &["/tutorial", "/tutorial guide:staff"]), ("/mypage", &[]), ("/panel", &[])],
    },
    Guide {
        name: "posting",
        audience: Audience::Everyone,
        summary: "Post as your character: prefixes, expressions, editing and deleting",
        steps: &[
            "1. On `/mypage`, under a character's **Posting**, give them a prefix, like `nova`.",
            "2. Post `nova: Hello there`: the bot posts it again under their name and picture, and removes yours.",
            "3. **Expressions** are other pictures: add `happy` with an image link or an upload, then post `nova:happy Hi!`. An unknown one posts with the main picture.",
            "React ❌ to your post to delete it, every part; react ✏️ and press **Edit** to change it. Only you can.",
            "A reply keeps who it answers; long posts come in parts; attachments come along.",
            "Staff post as the cast the same way: an NPC's prefix, when they have no character of their own with it.",
        ],
        commands: &[("/mypage", &[])],
    },
    Guide {
        name: "slots",
        audience: Audience::Everyone,
        summary: "Your character slots, and switching who you play",
        steps: &[
            "`/panel` shows every slot: race, tier, prefix, posts, what they carry; ✅ marks the one you play.",
            "Press a number under it to play that character.",
            "Faster: type `.set1` (`.set2` for slot 2 …); the answer and your command go away after 7 seconds.",
            "Or `/set character:` and pick from the menu. `/panel player:@someone` shows theirs.",
        ],
        commands: &[("/panel", &["/panel", "/panel player:@Nicolas"]), ("/set", &["/set character:Captain Nova"]), (DOT_SET, &[".set1"])],
    },
    Guide {
        name: "page",
        audience: Audience::Everyone,
        summary: "Your page: your characters, who you play, your Spirit Points",
        steps: &[
            "`/mypage` sends a link by DM; with DMs closed it shows only to you, here.",
            "The link works until 30 minutes pass without use, 4 hours at most. A new one replaces the old. Never share it.",
            "**My characters**: each as a card. **Play as <name>** under a card makes them the one you play.",
            "Under a card: **Picture** uploads its picture; **Expressions** takes a link or an upload. Staff see every upload in the mod log.",
            "**Spirit Points**: your balance and its last changes. Staff give them; posting earns none.",
            "**Shop**: spend Spirit Points; what you buy shows under **Inventory** (`/help shop`).",
            "**Players**: everyone with a character, and their cards.",
        ],
        commands: &[("/mypage", &["/mypage"])],
    },
    Guide {
        name: "shop",
        audience: Audience::Everyone,
        summary: "Spend Spirit Points in the shop; your inventory",
        steps: &[
            "1. `/mypage`, then **Shop** in the menu: every item, its price, and how many you have.",
            "2. Pick how many and press **Buy**. The Spirit Points go at once; the item lands in your **Inventory**.",
            "A free item is one each. Every purchase shows in your Spirit Points history and the mod log.",
            "Staff keep the shop and can set what anyone owns.",
        ],
        commands: &[("/mypage", &["/mypage"])],
    },
    Guide {
        name: "cards",
        audience: Audience::Everyone,
        summary: "Show a character's card in a channel",
        steps: &[
            "`/profile` alone shows the character you play.",
            "Name another of yours with `character:`, from the menu.",
            "Name a player with `player:` to show theirs.",
            "A card shows the character's picture, else the player's Discord picture.",
        ],
        commands: &[("/profile", &["/profile", "/profile character:Captain Nova", "/profile player:@Nicolas character:Captain Nova"])],
    },
    Guide {
        name: "mod-log",
        audience: Audience::Staff,
        summary: "Where every change is posted; nothing changes until it is set",
        steps: &[
            "1. Make a channel staff can read, and let the bot post there.",
            "2. `/z_moderator_dbrs action:mod_log channel:#mod-log`. The bot posts a line there first: a channel it cannot write to is refused.",
            "Every change, from Discord or the web, then lands there, with no pings. Web changes end with “· admin pages”.",
        ],
        commands: &[("/z_moderator_dbrs", &["/z_moderator_dbrs action:mod_log channel:#mod-log"])],
    },
    Guide {
        name: "admin",
        audience: Audience::Staff,
        summary: "The admin pages: players, characters, NPCs, McGuffins, Dragon Balls, worlds, the shop",
        steps: &[
            "`/z_moderator_admin` DMs you a link: 30 minutes without use, 4 hours at most; a new one replaces the old.",
            "**Players**: Spirit Points (give, take, set, with a memo), the ledger, add a character.",
            "**Characters** and **NPCs**: the sheet, a picture (link or upload), McGuffins carried, feats.",
            "**McGuffins**: make, edit, place, give, take back, retire, restore.",
            "**Dragon Balls**: the roll needed, search areas, hiding places, scatter.",
            "**Worlds**: which categories are worlds; their text channels are the roleplay channels.",
            "**Shop**: add, edit, hide or remove items. A player's page sets what they own, under **Inventory**; 0 takes it away.",
            "Never share the link. A player's /mypage link never opens these pages.",
        ],
        commands: &[("/z_moderator_admin", &["/z_moderator_admin"])],
    },
    Guide {
        name: "spirit",
        audience: Audience::Staff,
        summary: "Give, take, set or grant Spirit Points",
        steps: &[
            "Every change needs a memo: it is kept in the ledger and posted to the mod log.",
            "One player: `give`, `take` or `set`, here or on their admin page.",
            "Everyone holding a role: `grant`, here only.",
            "`history` shows a balance and its last lines.",
        ],
        commands: &[(
            "/z_moderator_sp",
            &[
                "/z_moderator_sp action:give player:@Nicolas amount:5 memo:Won the tournament",
                "/z_moderator_sp action:grant role:@Accepted amount:5 memo:Event",
                "/z_moderator_sp action:history player:@Nicolas",
            ],
        )],
    },
    Guide {
        name: "characters",
        audience: Audience::Staff,
        summary: "Add approved characters; edit sheets and NPCs",
        steps: &[
            "Add an approved character to a free slot: here, or **Add character** on the player's admin page.",
            "Sheets, pictures and NPCs are edited on the admin pages.",
            "A player picks who they play on their own page; staff do not.",
        ],
        commands: &[("/z_moderator_character", &["/z_moderator_character action:add player:@Nicolas slot:1 name:Captain Nova"]), ("/z_moderator_admin", &[])],
    },
    Guide {
        name: "feats",
        audience: Audience::Staff,
        summary: "Silent feats for tier-up reviews",
        steps: &[
            "A feat is a staff note on what a character did, linked to the posts that show it. Players never see them.",
            "Fastest: right-click the post › Apps › **Log feat**: the post and its author's character are filled in.",
            "Pick a feat from the `feat:` menu (F-0012 · Itami …) to edit, link, unlink, archive or restore it.",
            "Archive hides a feat; nothing deletes one. `list` with `state:archived` shows the archived.",
            "The same is on each character's admin page.",
        ],
        commands: &[
            (
                "/z_moderator_feat",
                &[
                    "/z_moderator_feat action:add player:@Nicolas character:Captain Nova kind:victory counts:fight note:Beat Retsu",
                    "/z_moderator_feat action:archive feat:F-0012 reason:duplicate",
                    "/z_moderator_feat action:list player:@Nicolas character:Captain Nova",
                ],
            ),
            (LOG_FEAT, &[]),
        ],
    },
    Guide {
        name: "mcguffins",
        audience: Audience::Staff,
        summary: "Plot items: where they lie, and who holds them",
        steps: &[
            "On the admin pages, **McGuffins**: make one with a name, an emoji, one holder or many, and what it is.",
            "**Where it lies**, from the server's channels as they are now: a world (a category), one channel, or a channel rolled at random from every roleplay channel. **Re-roll** picks again.",
            "Give it to a character, or take it back. A one-holder McGuffin moves to whoever gets it, and leaves its place.",
            "Retire hides it from cards; restore brings it back. Nothing deletes one.",
        ],
        commands: &[("/z_moderator_admin", &[])],
    },
    Guide {
        name: "worlds",
        audience: Audience::Staff,
        summary: "Which categories are worlds, and so which channels are roleplay",
        steps: &[
            "On the admin pages, **Worlds**: tick each category that is a world. The list is the server's, read when you open the page.",
            "A world's text channels are roleplay channels: a McGuffin placed at random lands in one of them.",
            "A new channel in a world counts at once; a category made later needs ticking.",
        ],
        commands: &[("/z_moderator_admin", &[])],
    },
    Guide {
        name: "dragon-balls",
        audience: Audience::Staff,
        summary: "Where each Dragon Ball set can be found, and where its balls hide",
        steps: &[
            "On the admin pages, **Dragon Balls**, then a set (Earth, Namek, Black Star, Spirit).",
            "**Roll needed**: what a search needs on 1d100.",
            "**Search areas**: tick a category (all its text channels, new ones too) or single channels.",
            "**Hide here** puts one ball in a channel; **Scatter** hides every loose ball over the areas. A held ball stays held.",
        ],
        commands: &[("/z_moderator_admin", &[])],
    },
    Guide {
        name: "prune",
        audience: Audience::Staff,
        summary: "Delete a channel's last messages",
        steps: &[
            "Type `.prune <count>` as a message in the channel: it deletes that many messages before it, then itself.",
            "Messages older than 14 days stay: Discord bulk-deletes none of those.",
            "The bot needs Manage Messages and Read Message History there.",
        ],
        commands: &[(PRUNE, &[".prune 10"])],
    },
    Guide {
        name: "trouble",
        audience: Audience::Staff,
        summary: "When something fails",
        steps: &[
            "A failed command says “That did not work (ref 123456)”. The bot's log has the rest: `journalctl -u noxadbrs -g 123456`.",
            "“Set the mod log first”: nothing changes until `/z_moderator_dbrs action:mod_log` is set.",
            "“This link has expired”: ask for a new one; links last 30 minutes without use.",
            "A player trying a staff command gets “Staff only.”, and the log says who.",
        ],
        commands: &[],
    },
];

fn type_word(kind: CommandOptionType) -> &'static str {
    match kind {
        CommandOptionType::Integer => "integer",
        CommandOptionType::Number => "number",
        CommandOptionType::Boolean => "true|false",
        CommandOptionType::User => "user",
        CommandOptionType::Channel => "channel",
        CommandOptionType::Role => "role",
        CommandOptionType::Mentionable => "user|role",
        CommandOptionType::Attachment => "file",
        _ => "text",
    }
}

fn choice_value(value: &CommandOptionChoiceValue) -> String {
    match value {
        CommandOptionChoiceValue::String(text) => text.clone(),
        CommandOptionChoiceValue::Integer(n) => n.to_string(),
        CommandOptionChoiceValue::Number(n) => n.to_string(),
    }
}

fn bound(value: Option<&CommandOptionValue>) -> Option<String> {
    value.map(|v| match v {
        CommandOptionValue::Integer(n) => n.to_string(),
        CommandOptionValue::Number(n) => n.to_string(),
    })
}

// `kind:victory|defeat`, or `amount:<integer>`.
fn term_of(option: &CommandOption) -> String {
    let choices = option.choices.as_deref().unwrap_or_default();
    let value = if !choices.is_empty() && choices.len() <= CHOICES_SHOWN {
        choices.iter().map(|c| choice_value(&c.value)).collect::<Vec<_>>().join("|")
    } else {
        format!("<{}>", type_word(option.kind))
    };
    format!("{}:{value}", option.name)
}

// "Whose Spirit Points · 0 to 1000 · suggests as you type"
fn meaning_of(option: &CommandOption, required: bool) -> String {
    let mut parts = vec![option.description.clone()];
    if required {
        parts.push("required".to_owned());
    }
    let (low, high) = (bound(option.min_value.as_ref()), bound(option.max_value.as_ref()));
    if low.is_some() || high.is_some() {
        parts.push(format!("{} to {}", low.unwrap_or_else(|| "…".to_owned()), high.unwrap_or_else(|| "…".to_owned())));
    }
    let choices = option.choices.as_deref().unwrap_or_default().len();
    if choices > CHOICES_SHOWN {
        parts.push(format!("one of {choices} choices"));
    }
    if option.autocomplete == Some(true) {
        parts.push("suggests as you type".to_owned());
    }
    parts.join(" · ")
}

fn is_staff_command(name: &str) -> bool {
    name.starts_with(STAFF_PREFIX) || name == LOG_FEAT
}

// A command's page, from its registration.
fn command_page(command: &Command) -> Page {
    if command.kind == CommandType::Message {
        let mut page = Page::new(command.name.clone(), Audience::Staff, Kind::Command, "Staff: log a feat from a post, its author's character filled in".to_owned());
        page.synopsis = vec![format!("Right-click a post › Apps › {}", command.name)];
        page.description = vec!["A form opens: pick the kind, what it counts as, and write what staff saw. The feat links to that post.".to_owned()];
        return page;
    }

    let audience = if is_staff_command(&command.name) { Audience::Staff } else { Audience::Everyone };
    let full = format!("{SLASH}{}", command.name);
    let mut page = Page::new(full.clone(), audience, Kind::Command, command.description.clone());
    let option = |name: &str| command.options.iter().find(|o| o.name == name);

    let Some(action) = option(ACTION) else {
        let terms: Vec<_> = command.options.iter().map(|o| if o.required == Some(true) { term_of(o) } else { format!("[{}]", term_of(o)) }).collect();
        page.synopsis = vec![[full.clone()].into_iter().chain(terms).collect::<Vec<_>>().join(" ")];
        page.options = command.options.iter().map(|o| Term { term: term_of(o), text: meaning_of(o, o.required == Some(true)) }).collect();
        return page;
    };

    let rows: Vec<_> = USAGE.iter().filter(|(c, _, _)| *c == command.name).collect();
    page.synopsis = rows
        .iter()
        .map(|(_, verb, takes)| {
            let terms = takes.iter().filter_map(|t| {
                let (name, optional) = t.strip_prefix(OPTIONAL).map_or((*t, false), |n| (n, true));
                let term = term_of(option(name)?);
                Some(if optional { format!("[{term}]") } else { term })
            });
            [format!("{full} {ACTION}:{verb}")].into_iter().chain(terms).collect::<Vec<_>>().join(" ")
        })
        .collect();
    page.actions = action.choices.as_deref().unwrap_or_default().iter().map(|c| Term { term: choice_value(&c.value), text: c.name.clone() }).collect();
    page.options = command
        .options
        .iter()
        .filter(|o| o.name != ACTION)
        .map(|o| {
            let verbs: Vec<_> = rows.iter().filter(|(_, _, takes)| takes.iter().any(|t| t.trim_start_matches(OPTIONAL) == o.name)).map(|(_, verb, _)| *verb).collect();
            Term { term: term_of(o), text: format!("{} · {}", meaning_of(o, false), verbs.join(", ")) }
        })
        .collect();
    page
}

fn prune_page(reading: Reading) -> Page {
    let mut page = Page::new(PRUNE.to_owned(), Audience::Staff, Kind::Command, "Staff: delete this channel's last messages".to_owned());
    page.synopsis = vec![format!("{PRUNE} <count>")];
    page.options = vec![Term { term: "<count>".to_owned(), text: format!("How many messages before it, 1 to {PRUNE_MAX}") }];
    page.description = vec![
        "Typed as a message, not a slash command. Deletes the messages before it, then itself; one older than 14 days stays.".to_owned(),
        "The confirmation goes away by itself; the mod log gets a line. Waits for the mod log, like every change.".to_owned(),
    ];
    if reading == Reading::CommandsOnly {
        page.description.push("**Off right now**: the bot may not read messages. Enable the Message Content intent (Developer Portal › Bot), then restart the bot.".to_owned());
    }
    page
}

// The dot commands everyone may type: .set<slot>, .cookie.
fn dot_pages(reading: Reading) -> Vec<Page> {
    let off = (reading == Reading::CommandsOnly).then(|| "**Off right now**: the bot may not read messages (the Message Content intent).".to_owned());
    let mut set = Page::new(DOT_SET.to_owned(), Audience::Everyone, Kind::Command, "Play the character in a slot, fast".to_owned());
    set.synopsis = vec![format!("{DOT_SET}<slot>")];
    set.options = vec![Term { term: "<slot>".to_owned(), text: "1 to 10, no space needed: `.set2`".to_owned() }];
    set.description = ["Typed as a message. Answers with the NOW PLAYING box; it and your message go after 7 seconds.".to_owned()].into_iter().chain(off.clone()).collect();
    let mut cookie = Page::new(COOKIE.to_owned(), Audience::Everyone, Kind::Command, "A fortune cookie, from Curie's Curious Cookies".to_owned());
    cookie.synopsis = vec![COOKIE.to_owned()];
    cookie.description = ["Typed as a message: one of 168 fortunes. Your message goes; the cookie stays.".to_owned()].into_iter().chain(off).collect();
    vec![set, cookie]
}

// A guide's first word names its command; "Log feat" is two words and one command.
fn command_of(term: &str) -> &str {
    if term == LOG_FEAT { term } else { term.split_whitespace().next().unwrap_or(term) }
}

/// Every page: the guides first, then every command's, each linked to the guides that name it.
fn pages(reading: Reading) -> Vec<Page> {
    let mut commands: Vec<Page> = commands::all().iter().map(command_page).collect();
    commands.push(prune_page(reading));
    commands.extend(dot_pages(reading));

    let guides: Vec<Page> = GUIDES
        .iter()
        .map(|g| {
            let mut page = Page::new(g.name.to_owned(), g.audience, Kind::Guide, g.summary.to_owned());
            page.description = g.steps.iter().map(|s| (*s).to_owned()).collect();
            for (term, examples) in g.commands {
                let Some(command) = commands.iter_mut().find(|c| c.name == command_of(term)) else {
                    continue;
                };
                page.commands.push(Term { term: (*term).to_owned(), text: command.summary.clone() });
                command.examples.extend(examples.iter().map(|e| (*e).to_owned()));
                command.see_also.push(format!("/help {}", g.name));
            }
            page
        })
        .collect();
    guides.into_iter().chain(commands).collect()
}

fn visible(pages: Vec<Page>, staff: Audience) -> Vec<Page> {
    pages.into_iter().filter(|p| p.audience == Audience::Everyone || staff == Audience::Staff).collect()
}

// A name as typed: no sigil, no staff prefix. "/z_moderator_sp" and "sp" read the same.
fn bare(name: &str) -> String {
    name.trim_start_matches(['/', '.']).trim_start_matches(STAFF_PREFIX).to_lowercase()
}

/// The page a query names, and the words after its name: "/z_moderator_sp give" → (sp, ["give"]).
fn find<'a>(pages: &'a [Page], query: &str) -> Option<(&'a Page, Vec<String>)> {
    let query = query.trim().to_lowercase();
    if let Some(page) = pages.iter().find(|p| p.name.to_lowercase() == query) {
        return Some((page, Vec::new()));
    }
    let mut words = query.split_whitespace().map(str::to_owned);
    let first = words.next()?;
    let wanted = bare(&first);
    let sigil = first.chars().next().filter(|c| matches!(c, '/' | '.'));
    let named = |p: &&Page| bare(&p.name) == wanted && sigil.is_none_or(|s| p.name.starts_with(s));
    let page = pages.iter().find(named).or_else(|| pages.iter().find(|p| bare(&p.name) == wanted))?;
    Some((page, words.collect()))
}

/// A command's page cut to one action: its line and the options it takes.
fn narrow(page: &Page, words: &[String]) -> Page {
    let Some(verb) = words.first() else {
        return page.clone();
    };
    if !page.actions.iter().any(|a| &a.term == verb) {
        return page.clone();
    }
    let marker = format!("{ACTION}:{verb}");
    let mut narrowed = page.clone();
    narrowed.synopsis.retain(|line| line.split_whitespace().any(|w| w == marker));
    narrowed.actions.retain(|a| &a.term == verb);
    narrowed.options.retain(|o| o.text.rsplit(" · ").next().is_some_and(|verbs| verbs.split(", ").any(|v| v == verb)));
    narrowed.examples.retain(|e| e.split_whitespace().any(|w| w == marker));
    narrowed
}

/// Up to 25 page names starting with, else holding, what was typed.
fn suggest(pages: &[Page], typed: &str) -> Vec<String> {
    let typed = bare(typed);
    let starts = pages.iter().filter(|p| bare(&p.name).starts_with(&typed));
    let holds = pages.iter().filter(|p| !bare(&p.name).starts_with(&typed) && bare(&p.name).contains(&typed));
    starts.chain(holds).map(|p| p.name.clone()).take(MENU_MAX).collect()
}

fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    format!("{}…", text.chars().take(max - 1).collect::<String>())
}

// Lines into pieces no longer than a field holds, split between lines.
fn chunks(lines: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for line in lines.iter().map(|l| cut(l, FIELD_MAX)) {
        if !current.is_empty() && current.chars().count() + line.chars().count() + 1 > FIELD_MAX {
            out.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(&line);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn term_lines(terms: &[Term]) -> Vec<String> {
    terms.iter().map(|t| if t.text.is_empty() { format!("`{}`", t.term) } else { format!("`{}` — {}", t.term, t.text) }).collect()
}

fn page_embed(page: &Page) -> Embed {
    const BLANK: &str = "\u{200b}";
    let title = cut(&if page.summary.is_empty() { page.name.clone() } else { format!("{} — {}", page.name, page.summary) }, TITLE_MAX);
    let synopsis = if page.synopsis.is_empty() { String::new() } else { format!("```\n{}\n```", page.synopsis.join("\n")) };
    let sections = [
        ("DESCRIPTION", page.description.clone()),
        ("COMMANDS", term_lines(&page.commands)),
        ("ACTIONS", term_lines(&page.actions)),
        ("OPTIONS", term_lines(&page.options)),
        ("EXAMPLES", page.examples.iter().map(|e| format!("`{e}`")).collect()),
        ("SEE ALSO", if page.see_also.is_empty() { Vec::new() } else { vec![page.see_also.iter().map(|s| format!("`{s}`")).collect::<Vec<_>>().join(" · ")] }),
    ];

    let mut embed = EmbedBuilder::new().title(title.clone()).color(ACCENT).description(synopsis.clone()).footer(EmbedFooterBuilder::new(NOTATION));
    let mut used = title.chars().count() + synopsis.chars().count() + NOTATION.len();
    let mut fields = 0;
    'sections: for (name, lines) in sections {
        for (i, value) in chunks(&lines).into_iter().enumerate() {
            let label = if i == 0 { name } else { BLANK };
            if fields + 1 >= FIELDS_MAX || used + label.len() + value.chars().count() > PAGE_ROOM {
                embed = embed.field(EmbedFieldBuilder::new(BLANK, "Too long for one page: `/help <command> <action>` shows one action."));
                break 'sections;
            }
            used += label.len() + value.chars().count();
            fields += 1;
            embed = embed.field(EmbedFieldBuilder::new(label, value));
        }
    }
    embed.build()
}

fn index_embeds(pages: &[Page]) -> Vec<Embed> {
    let section = |audience: Audience, title: &str, intro: &str| {
        let guides: Vec<_> = pages.iter().filter(|p| p.audience == audience && p.kind == Kind::Guide).map(|p| format!("`{}` — {}", p.name, p.summary)).collect();
        let mut names: Vec<_> = pages.iter().filter(|p| p.audience == audience && p.kind == Kind::Command).map(|p| format!("`{}`", p.name)).collect();
        names.sort();
        if guides.is_empty() && names.is_empty() {
            return None;
        }
        let body = format!("{intro}\n\n**Guides**\n{}\n\n**Commands**\n{}", guides.join("\n"), names.join(" · "));
        Some(EmbedBuilder::new().title(title).color(ACCENT).description(cut(&body, 4096)).build())
    };
    [
        section(
            Audience::Everyone,
            "📖 Help",
            "`/help <topic>`: a guide (`/help start`), a command (`/help /profile`), or one action (`/help /z_moderator_sp give`). `/tutorial` walks through it all.",
        ),
        section(Audience::Staff, "🛠️ Help for staff", "Only staff see these."),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn audience_of(ctx: &Ctx<'_>) -> Audience {
    if ctx.is_staff() { Audience::Staff } else { Audience::Everyone }
}

pub(crate) async fn show(ctx: &Ctx<'_>, opts: &Options<'_>) -> Result<()> {
    let pages = visible(pages(ctx.app.reading), audience_of(ctx));
    let Some(query) = opts.string(TOPIC).filter(|q| !q.trim().is_empty()) else {
        return ctx.whisper_embeds(index_embeds(&pages)).await;
    };
    let Some((page, words)) = find(&pages, query) else {
        let close = suggest(&pages, query).into_iter().take(5).map(|n| format!("`{n}`")).collect::<Vec<_>>();
        let hint = if close.is_empty() { String::new() } else { format!(" Close: {}.", close.join(" · ")) };
        return ctx.whisper(&format!("No help for `{}`.{hint} `/help` lists everything.", cut(query.trim(), CHOICE_MAX))).await;
    };
    ctx.whisper_embeds(vec![page_embed(&narrow(page, &words))]).await
}

/// The topic menu: the pages the caller may read.
pub(crate) async fn complete(ctx: &Ctx<'_>, opts: &Options<'_>) -> Result<()> {
    let typed = opts.focused().map(|(_, text)| text).unwrap_or_default();
    let pages = visible(pages(ctx.app.reading), audience_of(ctx));
    let choices = suggest(&pages, typed)
        .into_iter()
        .map(|name| {
            let summary = pages.iter().find(|p| p.name == name).map(|p| p.summary.clone()).unwrap_or_default();
            (cut(&format!("{name} — {summary}"), CHOICE_MAX), name)
        })
        .collect();
    ctx.choices(choices).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use twilight_validate::embed::embed;

    fn size(e: &Embed) -> usize {
        let fields: usize = e.fields.iter().map(|f| f.name.chars().count() + f.value.chars().count()).sum();
        e.title.as_deref().map_or(0, |t| t.chars().count()) + e.description.as_deref().map_or(0, |d| d.chars().count()) + e.footer.as_ref().map_or(0, |f| f.text.chars().count()) + fields
    }

    #[test]
    fn every_command_and_every_page_fits_discord() {
        let all = pages(Reading::Messages);
        for command in commands::all() {
            let name = if command.kind == CommandType::Message { command.name.clone() } else { format!("/{}", command.name) };
            assert!(all.iter().any(|p| p.name == name), "no page for {name}");
        }
        for page in &all {
            let e = page_embed(page);
            assert!(embed(&e).is_ok(), "{} breaks an embed limit", page.name);
            assert!(size(&e) <= MESSAGE_EMBEDS_MAX, "{} is too long", page.name);
        }
        let index = index_embeds(&all);
        assert!(index.iter().all(|e| embed(e).is_ok()));
        assert!(index.iter().map(size).sum::<usize>() <= MESSAGE_EMBEDS_MAX);
    }

    #[test]
    fn usage_names_only_actions_and_options_discord_has_and_misses_no_action() {
        for command in commands::all() {
            let Some(action) = command.options.iter().find(|o| o.name == ACTION) else {
                continue;
            };
            for choice in action.choices.as_deref().unwrap_or_default() {
                let verb = choice_value(&choice.value);
                assert!(USAGE.iter().any(|(c, v, _)| *c == command.name && *v == verb), "{} {verb} has no usage line", command.name);
            }
        }
        for (name, verb, takes) in USAGE {
            let command = commands::all().into_iter().find(|c| c.name == *name).unwrap();
            let choices = command.options.iter().find(|o| o.name == ACTION).and_then(|o| o.choices.clone()).unwrap_or_default();
            assert!(choices.iter().any(|c| choice_value(&c.value) == *verb), "{name} has no action {verb}");
            for option in *takes {
                let option = option.trim_start_matches(OPTIONAL);
                assert!(command.options.iter().any(|o| o.name == option), "{name} has no option {option}");
            }
        }
    }

    #[test]
    fn every_guide_names_real_commands() {
        let all = pages(Reading::Messages);
        for guide in GUIDES {
            for (term, _) in guide.commands {
                assert!(all.iter().any(|p| p.kind == Kind::Command && p.name == command_of(term)), "{} names {term}", guide.name);
            }
        }
    }

    #[test]
    fn players_never_see_staff_pages() {
        let players = visible(pages(Reading::Messages), Audience::Everyone);
        assert!(players.iter().all(|p| p.audience == Audience::Everyone));
        assert!(find(&players, "/z_moderator_sp").is_none());
        assert!(find(&players, ".prune").is_none());
        assert!(suggest(&players, "z_").is_empty());
        assert_eq!(index_embeds(&players).len(), 1);
        assert!(find(&players, "/profile").is_some());
    }

    #[test]
    fn a_page_is_found_by_its_name_loosely_and_narrowed_to_one_action() {
        let all = pages(Reading::Messages);
        assert_eq!(find(&all, "/z_moderator_sp").unwrap().0.name, "/z_moderator_sp");
        assert_eq!(find(&all, "SP").unwrap().0.name, "/z_moderator_sp");
        assert_eq!(find(&all, "log feat").unwrap().0.name, LOG_FEAT);
        assert_eq!(find(&all, ".prune").unwrap().0.name, PRUNE);
        assert_eq!(find(&all, "feats").unwrap().0.kind, Kind::Guide);
        assert!(find(&all, "teleport").is_none());

        let (page, words) = find(&all, "/z_moderator_sp grant").unwrap();
        let grant = narrow(page, &words);
        assert_eq!(grant.synopsis, ["/z_moderator_sp action:grant role:<role> amount:<integer> memo:<text>"]);
        assert!(grant.options.iter().any(|o| o.term == "role:<role>"));
        assert!(!grant.options.iter().any(|o| o.term == "player:<user>"));
        assert!(grant.examples.iter().all(|e| e.contains("action:grant")));
        assert!(page.see_also.contains(&"/help spirit".to_owned()));
    }

    #[test]
    fn prune_says_when_it_is_off() {
        assert!(prune_page(Reading::CommandsOnly).description.iter().any(|d| d.contains("Off right now")));
        assert!(!prune_page(Reading::Messages).description.iter().any(|d| d.contains("Off right now")));
    }
}
