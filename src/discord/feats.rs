//! Feats: silent staff records of what a character did, linked to the posts that show it.
//!
//! ```text
//!   /z_moderator_feat action:add player character kind counts note [link]
//!   /z_moderator_feat action:edit feat [kind] [counts] [note]
//!   /z_moderator_feat action:link feat link · action:unlink feat link
//!   /z_moderator_feat action:archive feat [reason] · action:restore feat   never deleted: archive hides
//!   /z_moderator_feat action:list player character [state]
//!   right-click a post › Apps › Log feat                     the post and its author's character prefilled
//! ```
//!
//! Every change waits for the mod log and lands there. Players are never told.

use anyhow::Result;
use twilight_model::application::interaction::application_command::CommandData;
use twilight_model::application::interaction::modal::{ModalInteractionComponent, ModalInteractionData};
use twilight_model::channel::message::component::{
    Component, Label, SelectMenu, SelectMenuOption, SelectMenuType, TextInput, TextInputStyle,
};
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, MessageMarker, UserMarker};

use super::characters::{menu as character_menu, pick};
use super::ctx::{Ctx, Options};
use super::modlog::{self, NO_MOD_LOG};
use super::text::{capitalized, changes_of, cut, feat_block, menu_line, plain, post_link};
use crate::domain::feats::{FeatCounts, FeatKind, FeatState, NOTE_MAX, feat_id_of};
use crate::domain::links::{MessageLink, parse_link};
use crate::services::feats::{Added, Edited, FeatEdit, FeatView, Linked, Moved, NewFeat, PostRef, Unlinked};

const MENU_LIMIT: usize = 25;
const CHOICE_MAX: usize = 100;
const REPLY_MAX: usize = 2000;
const MODAL_TITLE_MAX: usize = 45;
// "noxadbrs|feat|<channel>|<message>|<character>": the Log feat form for that post and character.
const FORM_PREFIX: &str = "noxadbrs|feat|";

const NOT_A_LINK: &str = "That is not a message link. Right-click the post › Copy Message Link.";
const OTHER_SERVER: &str = "That post is on another server.";
const MISSING_POST: &str = "That post is gone, or in a channel I cannot read.";
const NO_FEAT: &str = "No such feat: pick one from the menu.";
const IS_ARCHIVED: &str = "That feat is archived. Restore it first.";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FeatSub {
    Add,
    Edit,
    Link,
    Unlink,
    Archive,
    Restore,
    List,
}

impl FeatSub {
    pub(crate) const ALL: [Self; 7] = [Self::Add, Self::Edit, Self::Link, Self::Unlink, Self::Archive, Self::Restore, Self::List];

    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Edit => "edit",
            Self::Link => "link",
            Self::Unlink => "unlink",
            Self::Archive => "archive",
            Self::Restore => "restore",
            Self::List => "list",
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Add => "Add: log a feat (player, character, kind, counts, note)",
            Self::Edit => "Edit: change a feat's kind, count or note",
            Self::Link => "Link: add a post to a feat",
            Self::Unlink => "Unlink: remove a post from a feat",
            Self::Archive => "Archive: hide a feat; restore brings it back",
            Self::Restore => "Restore: bring an archived feat back",
            Self::List => "List: a character's feats",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|sub| sub.key() == key)
    }
}

/// The form's fields, also the slash options of the same name.
#[derive(Clone, Copy)]
pub(crate) enum FormField {
    Kind,
    Counts,
    Note,
}

impl FormField {
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Kind => "kind",
            Self::Counts => "counts",
            Self::Note => "note",
        }
    }
}

// The post a pasted link names, checked: a message link, on this server, still there.
async fn post_of(ctx: &Ctx<'_>, text: &str) -> Result<PostRef, &'static str> {
    let Some(link) = parse_link(text) else {
        return Err(NOT_A_LINK);
    };
    if link.guild_id != ctx.app.guild_id.get() {
        return Err(OTHER_SERVER);
    }
    let (Some(channel), Some(message)) = (Id::<ChannelMarker>::new_checked(link.channel_id), Id::<MessageMarker>::new_checked(link.message_id)) else {
        return Err(NOT_A_LINK);
    };
    if ctx.app.http.message(channel, message).await.is_err() {
        return Err(MISSING_POST);
    }
    Ok(PostRef { channel_id: link.channel_id, message_id: link.message_id })
}

pub(crate) async fn admin(ctx: &Ctx<'_>, opts: &Options<'_>) -> Result<()> {
    if !ctx.is_staff() {
        return ctx.deny().await;
    }
    ctx.defer().await?;

    let Some(staff) = ctx.user_id() else {
        return Ok(());
    };
    let Some(sub) = opts.sub().and_then(FeatSub::from_key) else {
        return ctx.edit("Pick an action.").await;
    };
    if sub == FeatSub::List {
        return list(ctx, opts).await;
    }
    let Some(log) = modlog::required(ctx.app)? else {
        return ctx.edit(NO_MOD_LOG).await;
    };

    let said = match sub {
        FeatSub::Add => add(ctx, opts, staff).await?,
        FeatSub::Edit => edit(ctx, opts, staff)?,
        FeatSub::Link => link(ctx, opts, staff).await?,
        FeatSub::Unlink => unlink(ctx, opts)?,
        FeatSub::Archive => archive(ctx, opts, staff)?,
        FeatSub::Restore => restore(ctx, opts, staff)?,
        FeatSub::List => unreachable!("handled above"),
    };
    logged(ctx, log, staff, said).await
}

/// A change's outcome: a refusal to say, or the line to log and say.
enum Said {
    Refused(String),
    Done(String),
}

async fn logged(ctx: &Ctx<'_>, log: Id<ChannelMarker>, staff: Id<UserMarker>, said: Said) -> Result<()> {
    let line = match said {
        Said::Refused(why) => return ctx.edit(&why).await,
        Said::Done(line) => line,
    };
    modlog::post(ctx.app, log, &format!("🔒 <@{staff}> {line}{}", modlog::where_of(ctx.channel_id()))).await;
    ctx.edit(&cut(&format!("🔒 {}", capitalized(&line)), REPLY_MAX)).await
}

fn no(why: &str) -> Said {
    Said::Refused(why.to_owned())
}

fn feat_id(opts: &Options<'_>) -> i64 {
    opts.string("feat").and_then(feat_id_of).unwrap_or_default()
}

async fn add(ctx: &Ctx<'_>, opts: &Options<'_>, staff: Id<UserMarker>) -> Result<Said> {
    let Some(player) = opts.user("player") else {
        return Ok(no("Name a player."));
    };
    let theirs = ctx.app.services.characters().of_player(player.get())?;
    let Some(character) = pick(&theirs, opts.string("character").unwrap_or_default()) else {
        return Ok(no(&format!("Pick one of <@{player}>'s characters from the menu.")));
    };
    let (Some(kind), Some(counts)) = (
        opts.string(FormField::Kind.key()).and_then(FeatKind::from_key),
        opts.string(FormField::Counts.key()).and_then(FeatCounts::from_key),
    ) else {
        return Ok(no("Pick a kind and what it counts as."));
    };
    let post = match opts.string("link") {
        Some(text) => match post_of(ctx, text).await {
            Ok(post) => Some(post),
            Err(why) => return Ok(no(why)),
        },
        None => None,
    };

    let new = NewFeat { character_id: character.id, kind, counts, note: opts.string(FormField::Note.key()).unwrap_or_default(), post, staff_id: staff.get() };
    Ok(added(ctx, ctx.app.services.feats().add(&new)?))
}

fn added(ctx: &Ctx<'_>, outcome: Added) -> Said {
    match outcome {
        Added::Done(feat) => Said::Done(format!("logged {}", feat_block(ctx.app.guild_id.get(), &feat))),
        Added::NoNote => no("Write a note: what happened."),
        Added::NoCharacter => no("That character is gone."),
    }
}

fn edit(ctx: &Ctx<'_>, opts: &Options<'_>, staff: Id<UserMarker>) -> Result<Said> {
    let edit = FeatEdit {
        id: feat_id(opts),
        kind: opts.string(FormField::Kind.key()).and_then(FeatKind::from_key),
        counts: opts.string(FormField::Counts.key()).and_then(FeatCounts::from_key),
        note: opts.string(FormField::Note.key()),
        staff_id: staff.get(),
    };
    Ok(match ctx.app.services.feats().edit(&edit)? {
        Edited::Done { before, after } => Said::Done(format!("edited **{}** ({}): {}", after.code, plain(&after.character_name), changes_of(&before, &after))),
        Edited::NotFound => no(NO_FEAT),
        Edited::Archived => no(IS_ARCHIVED),
        Edited::NoNote => no("Write a note: what happened."),
        Edited::NoChange => no("Nothing to change: give a new kind, count or note."),
    })
}

async fn link(ctx: &Ctx<'_>, opts: &Options<'_>, staff: Id<UserMarker>) -> Result<Said> {
    let post = match post_of(ctx, opts.string("link").unwrap_or_default()).await {
        Ok(post) => post,
        Err(why) => return Ok(no(why)),
    };
    let guild = ctx.app.guild_id.get();
    Ok(match ctx.app.services.feats().link(feat_id(opts), post, staff.get())? {
        Linked::Done(feat) => Said::Done(format!("linked {} to **{}** ({})", post_link(guild, post, "post"), feat.code, plain(&feat.character_name))),
        Linked::NotFound => no(NO_FEAT),
        Linked::Archived => no(IS_ARCHIVED),
        Linked::Known => no("That feat links that post already."),
        Linked::Full => no("That feat has as many links as it can hold. Unlink one first."),
    })
}

fn unlink(ctx: &Ctx<'_>, opts: &Options<'_>) -> Result<Said> {
    let link_id = opts.string("link").and_then(|text| text.parse().ok()).unwrap_or_default();
    let guild = ctx.app.guild_id.get();
    Ok(match ctx.app.services.feats().unlink(feat_id(opts), link_id)? {
        Unlinked::Done { feat, removed } => {
            Said::Done(format!("unlinked {} from **{}** ({})", post_link(guild, removed, "post"), feat.code, plain(&feat.character_name)))
        }
        Unlinked::NotFound => no(NO_FEAT),
        Unlinked::NoLink => no("That feat has no such link: pick one from the menu."),
    })
}

fn archive(ctx: &Ctx<'_>, opts: &Options<'_>, staff: Id<UserMarker>) -> Result<Said> {
    let reason = opts.string("reason");
    Ok(match ctx.app.services.feats().archive(feat_id(opts), reason, staff.get())? {
        Moved::Done(feat) => {
            let why = reason.map(|r| format!(" · {}", plain(r.trim()))).unwrap_or_default();
            Said::Done(format!("archived **{}** ({}){why}. `/z_moderator_feat restore` brings it back.", feat.code, plain(&feat.character_name)))
        }
        Moved::NotFound => no(NO_FEAT),
        Moved::Already => no(IS_ARCHIVED),
    })
}

fn restore(ctx: &Ctx<'_>, opts: &Options<'_>, staff: Id<UserMarker>) -> Result<Said> {
    Ok(match ctx.app.services.feats().restore(feat_id(opts), staff.get())? {
        Moved::Done(feat) => Said::Done(format!("restored **{}** ({})", feat.code, plain(&feat.character_name))),
        Moved::NotFound => no(NO_FEAT),
        Moved::Already => no("That feat is not archived."),
    })
}

// Blocks that fit one message, oldest first; the rest are counted.
fn fitted(header: &str, blocks: &[String]) -> String {
    let mut out = header.to_owned();
    let mut shown = 0;
    for block in blocks {
        if out.len() + block.len() + 2 > REPLY_MAX - CHOICE_MAX {
            break;
        }
        out.push_str("\n\n");
        out.push_str(block);
        shown += 1;
    }
    if shown < blocks.len() {
        out.push_str(&format!("\n\n… {} more.", blocks.len() - shown));
    }
    out
}

// A character's feats; read-only, so the mod log is not needed.
async fn list(ctx: &Ctx<'_>, opts: &Options<'_>) -> Result<()> {
    let Some(player) = opts.user("player") else {
        return ctx.edit("Name a player.").await;
    };
    let theirs = ctx.app.services.characters().of_player(player.get())?;
    let Some(character) = pick(&theirs, opts.string("character").unwrap_or_default()) else {
        return ctx.edit(&format!("Pick one of <@{player}>'s characters from the menu.")).await;
    };
    let state = opts.string("state").and_then(FeatState::from_key).unwrap_or(FeatState::Active);
    let feats = ctx.app.services.feats().list(character.id, state)?;

    let plural = if feats.len() == 1 { "" } else { "s" };
    let header = format!("🔒 {} · {} {} feat{plural}", plain(&character.name), feats.len(), state.key());
    let blocks: Vec<_> = feats.iter().map(|f| feat_block(ctx.app.guild_id.get(), f)).collect();
    ctx.edit(&fitted(&header, &blocks)).await
}

/// The menus: a player's characters, feats by state, and one feat's links.
pub(crate) async fn complete(ctx: &Ctx<'_>, opts: &Options<'_>) -> Result<()> {
    if !ctx.is_staff() {
        return ctx.choices(Vec::new()).await;
    }
    let Some((name, typed)) = opts.focused() else {
        return ctx.choices(Vec::new()).await;
    };

    let choices = match name {
        "character" => {
            let theirs = match opts.user("player") {
                Some(player) => ctx.app.services.characters().of_player(player.get())?,
                None => Vec::new(),
            };
            character_menu(&theirs, typed)
        }
        // Unlink picks one of the feat's posts; add and link take a pasted message link, echoed back.
        "link" if opts.sub() == Some(FeatSub::Unlink.key()) => {
            let feat = ctx.app.services.feats().get(feat_id(opts))?;
            let links = feat.map(|f: FeatView| f.links).unwrap_or_default();
            (1..).zip(links).take(MENU_LIMIT).map(|(n, l)| (format!("post {n} · message {}", l.message_id), l.id.to_string())).collect()
        }
        "link" => match typed.trim() {
            "" => Vec::new(),
            pasted => vec![(cut(pasted, CHOICE_MAX), cut(pasted, CHOICE_MAX))],
        },
        _ => {
            let state = if opts.sub() == Some(FeatSub::Restore.key()) { FeatState::Archived } else { FeatState::Active };
            let items = ctx.app.services.feats().menu(state, typed)?;
            items.into_iter().map(|i| (menu_line(&i.code, &i.character_name, i.kind, &i.note, CHOICE_MAX), i.id.to_string())).collect()
        }
    };
    ctx.choices(choices).await
}

fn select(field: FormField, label: &str, placeholder: &str, options: Vec<SelectMenuOption>) -> Component {
    let menu = SelectMenu {
        channel_types: None,
        custom_id: field.key().to_owned(),
        default_values: None,
        disabled: false,
        id: None,
        kind: SelectMenuType::Text,
        max_values: Some(1),
        min_values: Some(1),
        options: Some(options),
        placeholder: Some(placeholder.to_owned()),
        required: Some(true),
    };
    Component::Label(Label { component: Box::new(Component::SelectMenu(menu)), description: None, id: None, label: label.to_owned() })
}

fn option(label: &str, value: &str) -> SelectMenuOption {
    SelectMenuOption { default: false, description: None, emoji: None, label: label.to_owned(), value: value.to_owned() }
}

// TextInput's own label is deprecated in favour of the Label around it; it is left empty.
#[allow(deprecated)]
fn note_input() -> Component {
    let input = TextInput {
        custom_id: FormField::Note.key().to_owned(),
        id: None,
        label: None,
        max_length: Some(NOTE_MAX as u16),
        min_length: Some(1),
        placeholder: Some("What staff saw".to_owned()),
        required: Some(true),
        style: TextInputStyle::Paragraph,
        value: None,
    };
    Component::Label(Label { component: Box::new(Component::TextInput(input)), description: None, id: None, label: "Note".to_owned() })
}

/// Right-click a post › Apps › Log feat. The author's active character, or how to log it by hand.
pub(crate) async fn log_feat(ctx: &Ctx<'_>, data: &CommandData) -> Result<()> {
    if !ctx.is_staff() {
        return ctx.deny().await;
    }
    if modlog::required(ctx.app)?.is_none() {
        return ctx.whisper(NO_MOD_LOG).await;
    }
    let target = data.target_id.map(|id| id.cast::<MessageMarker>());
    let Some(message) = target.and_then(|id| data.resolved.as_ref()?.messages.get(&id)) else {
        return ctx.whisper("I cannot see that post.").await;
    };

    let url = MessageLink { guild_id: ctx.app.guild_id.get(), channel_id: message.channel_id.get(), message_id: message.id.get() }.url();
    let by_hand = format!("`/z_moderator_feat add` with link:{url}");
    // A post as a character comes from a webhook: whose it is will be known once noxaDBRS proxies posts.
    if message.webhook_id.is_some() {
        return ctx.whisper(&format!("That is a post as a character. Log it with {by_hand}")).await;
    }
    let Some(character) = ctx.app.services.characters().active(message.author.id.get())? else {
        return ctx.whisper(&format!("<@{}> plays no character right now. Log it with {by_hand}", message.author.id)).await;
    };

    let kinds = FeatKind::ALL.iter().map(|k| option(k.label(), k.key())).collect();
    let counts = FeatCounts::ALL.iter().map(|c| option(c.label(), c.key())).collect();
    let components = vec![
        select(FormField::Kind, "Kind", "What happened", kinds),
        select(FormField::Counts, "Counts as", "What it stands for in a tier-up", counts),
        note_input(),
    ];
    let custom_id = format!("{FORM_PREFIX}{}|{}|{}", message.channel_id, message.id, character.id);
    ctx.modal(custom_id, cut(&format!("Log feat · {}", character.name), MODAL_TITLE_MAX), components).await
}

// A submitted field by its custom id, inside labels or action rows.
fn field<'a>(components: &'a [ModalInteractionComponent], id: &str) -> Option<&'a str> {
    components.iter().find_map(|component| match component {
        ModalInteractionComponent::Label(label) => field(std::slice::from_ref(&label.component), id),
        ModalInteractionComponent::ActionRow(row) => field(&row.components, id),
        ModalInteractionComponent::StringSelect(select) if select.custom_id == id => select.values.first().map(String::as_str),
        ModalInteractionComponent::TextInput(input) if input.custom_id == id => Some(input.value.as_str()),
        _ => None,
    })
}

/// The Log feat form sent: the feat logged on that post, as /z_moderator_feat add would.
pub(crate) async fn submit(ctx: &Ctx<'_>, data: &ModalInteractionData) -> Result<()> {
    let Some(rest) = data.custom_id.strip_prefix(FORM_PREFIX) else {
        return Ok(());
    };
    if !ctx.is_staff() {
        return ctx.deny().await;
    }
    ctx.defer().await?;
    let (Some(log), Some(staff)) = (modlog::required(ctx.app)?, ctx.user_id()) else {
        return ctx.edit(NO_MOD_LOG).await;
    };

    let ids: Vec<i64> = rest.split('|').filter_map(|part| part.parse().ok()).collect();
    let [channel_id, message_id, character_id] = ids[..] else {
        return Ok(());
    };
    let (Some(kind), Some(counts)) = (
        field(&data.components, FormField::Kind.key()).and_then(FeatKind::from_key),
        field(&data.components, FormField::Counts.key()).and_then(FeatCounts::from_key),
    ) else {
        return ctx.edit("Pick a kind and what it counts as.").await;
    };

    let post = PostRef { channel_id: crate::store::from_sql(channel_id), message_id: crate::store::from_sql(message_id) };
    let new = NewFeat { character_id, kind, counts, note: field(&data.components, FormField::Note.key()).unwrap_or_default(), post: Some(post), staff_id: staff.get() };
    let said = added(ctx, ctx.app.services.feats().add(&new)?);
    logged(ctx, log, staff, said).await
}
