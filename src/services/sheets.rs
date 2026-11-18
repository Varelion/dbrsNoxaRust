//! Character sheets, for staff: any character's, and new NPCs (the cast's characters). Every save
//! returns the sheet before and after, so the change can be logged field by field.
//!
//! ```text
//!   save(NewNpc, form)       ─► a cast character in the next free slot
//!   save(Existing(id), form) ─► that character, a player's or the cast's
//!   set_picture(id, upload)  ─► an uploaded picture replaces any link
//! ```

use anyhow::Result;

use crate::domain::npcs::CAST;
use crate::domain::proxy::{BadPrefix, clean_expression, clean_prefix, parse_color};
use crate::store::expressions::{self, ExpressionRow};
use crate::store::characters::{self as repo, CharacterRow, NewCharacter, SheetFields};
use crate::store::Store;

use super::characters::NAME_MAX;
use super::{Clock, clean_line};

const TEXT_MAX: usize = 1000;
const URL_MAX: usize = 400;
const TIER_MAX: i64 = 99;

/// What the sheet's picture becomes on save.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PictureChoice {
    /// The link typed in. Blank keeps an uploaded picture, else falls back to the owner's icon.
    #[default]
    Link,
    /// The owner's Discord icon: any link or upload is dropped.
    Icon,
}

/// Whether "prefix: text" posts as a character.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Posting {
    #[default]
    ByPrefix,
    Off,
}

impl Posting {
    pub(crate) const fn on(self) -> bool {
        matches!(self, Self::ByPrefix)
    }
}

/// A player's own posting settings, saved.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PostingSaved {
    Done(Box<CharacterRow>),
    NotYours,
    BadPrefix(&'static str),
    PrefixTaken(String),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ExpressionSaved {
    Done(ExpressionRow),
    NotFound,
    BadName,
    BadLink,
    BadColor,
}

/// Why an expression's name or colour is refused, checked before its image is uploaded; None: fine.
pub(crate) fn expression_refused(name: &str, color: &str) -> Option<ExpressionSaved> {
    if clean_expression(name).is_none() {
        return Some(ExpressionSaved::BadName);
    }
    parse_color(color).is_err().then_some(ExpressionSaved::BadColor)
}

/// Which sheet a save writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    NewNpc,
    Existing(i64),
}

/// A sheet as a form sends it: every field as text.
#[derive(Default)]
pub(crate) struct SheetForm {
    pub(crate) name: String,
    pub(crate) prefix: String,
    pub(crate) race: String,
    pub(crate) tier: String,
    pub(crate) title: String,
    pub(crate) bio: String,
    pub(crate) picture_url: String,
    pub(crate) picture: PictureChoice,
    pub(crate) notes: String,
    pub(crate) posting: Posting,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SheetSaved {
    Done { before: Option<Box<CharacterRow>>, after: Box<CharacterRow> },
    NotFound,
    NoName,
    BadPrefix(&'static str),
    PrefixTaken(String),
    BadTier,
    BadPicture,
}

fn optional(text: &str, max: usize) -> Option<String> {
    let kept: String = text.trim().chars().take(max).collect();
    (!kept.is_empty()).then_some(kept)
}

/// The fields that differ, by name: "race, bio".
pub(crate) fn changed(before: &CharacterRow, after: &CharacterRow) -> Vec<&'static str> {
    let pairs = [
        ("name", before.name != after.name),
        ("prefix", before.prefix != after.prefix),
        ("race", before.race != after.race),
        ("tier", before.tier != after.tier),
        ("title", before.title != after.title),
        ("bio", before.bio != after.bio),
        ("picture", before.picture_url != after.picture_url || before.picture_message != after.picture_message),
        ("notes", before.notes != after.notes),
        ("posting", before.posting != after.posting),
    ];
    pairs.into_iter().filter(|(_, differs)| *differs).map(|(name, _)| name).collect()
}

// The picture a save leaves: (link, uploaded message). A typed link replaces an upload.
fn picture_after(choice: PictureChoice, typed: Option<String>, before: Option<&CharacterRow>) -> (Option<String>, Option<u64>) {
    let upload = before.and_then(|row| Some((row.picture_url.clone(), row.picture_message?)));
    match (choice, typed, upload) {
        (PictureChoice::Icon, _, _) => (None, None),
        (PictureChoice::Link, Some(link), _) => (Some(link), None),
        (PictureChoice::Link, None, Some((url, message))) => (url, Some(message)),
        (PictureChoice::Link, None, None) => (None, None),
    }
}

pub(crate) struct Sheets<'a> {
    store: &'a Store,
    clock: &'a Clock,
}

impl<'a> Sheets<'a> {
    pub(crate) const fn new(store: &'a Store, clock: &'a Clock) -> Self {
        Self { store, clock }
    }

    /// The cast, by name.
    pub(crate) fn npcs(&self) -> Result<Vec<CharacterRow>> {
        let mut cast = self.store.read(|c| repo::of_player(c, CAST))?;
        cast.sort_by_key(|npc| npc.name.to_lowercase());
        Ok(cast)
    }

    /// Every character, by owner then slot: the cast (player 0) first.
    pub(crate) fn all(&self) -> Result<Vec<CharacterRow>> {
        self.store.read(repo::all)
    }

    pub(crate) fn get(&self, id: i64) -> Result<Option<CharacterRow>> {
        self.store.read(|c| repo::get(c, id))
    }

    /// An uploaded picture becomes the sheet's; None when the character is gone.
    pub(crate) fn set_picture(&self, id: i64, url: &str, message_id: u64) -> Result<Option<CharacterRow>> {
        self.store.write(|tx| {
            if !repo::set_picture(tx, id, url, message_id)? {
                return Ok(None);
            }
            repo::get(tx, id)
        })
    }

    /// A player's own character, or None.
    pub(crate) fn owned(&self, player: u64, id: i64) -> Result<Option<CharacterRow>> {
        Ok(self.get(id)?.filter(|row| row.player_id == player))
    }

    /// A character's prefix and whether it posts, set by its player (or staff for the cast: player 0).
    pub(crate) fn set_posting(&self, player: u64, id: i64, prefix: &str, posting: Posting) -> Result<PostingSaved> {
        let prefix = match clean_prefix(prefix) {
            Ok(prefix) => prefix,
            Err(bad) => return Ok(PostingSaved::BadPrefix(bad.reason())),
        };
        self.store.write(|tx| {
            if repo::get(tx, id)?.filter(|row| row.player_id == player).is_none() {
                return Ok(PostingSaved::NotYours);
            }
            if let Some(prefix) = prefix.as_deref()
                && let Some(other) = repo::by_prefix(tx, player, prefix)?.filter(|o| o.id != id)
            {
                return Ok(PostingSaved::PrefixTaken(other.name));
            }
            repo::set_posting(tx, id, prefix.as_deref(), posting.on())?;
            Ok(PostingSaved::Done(Box::new(repo::get(tx, id)?.expect("the character just saved"))))
        })
    }

    pub(crate) fn expressions(&self, id: i64) -> Result<Vec<ExpressionRow>> {
        self.store.read(|c| expressions::of_character(c, id))
    }

    /// Adds an expression, or replaces the one of that name.
    pub(crate) fn save_expression(&self, id: i64, name: &str, url: &str, color: &str) -> Result<ExpressionSaved> {
        let Some(name) = clean_expression(name) else {
            return Ok(ExpressionSaved::BadName);
        };
        let url = url.trim();
        if !url.starts_with("https://") || url.len() > URL_MAX {
            return Ok(ExpressionSaved::BadLink);
        }
        let Ok(color) = parse_color(color) else {
            return Ok(ExpressionSaved::BadColor);
        };
        let row = ExpressionRow { name, url: url.to_owned(), color };
        self.store.write(|tx| {
            if repo::get(tx, id)?.is_none() {
                return Ok(ExpressionSaved::NotFound);
            }
            expressions::save(tx, id, &row)?;
            Ok(ExpressionSaved::Done(row))
        })
    }

    /// False when the character had no such expression.
    pub(crate) fn remove_expression(&self, id: i64, name: &str) -> Result<bool> {
        let name = name.trim().to_lowercase();
        self.store.write(|tx| expressions::remove(tx, id, &name))
    }

    /// A new NPC or an edited sheet: checked whole, then written in one go.
    pub(crate) fn save(&self, target: Target, form: &SheetForm, staff_id: u64) -> Result<SheetSaved> {
        let name = clean_line(&form.name, NAME_MAX);
        if name.is_empty() {
            return Ok(SheetSaved::NoName);
        }
        let prefix = match clean_prefix(&form.prefix) {
            Ok(prefix) => prefix,
            Err(bad) => return Ok(SheetSaved::BadPrefix(BadPrefix::reason(&bad))),
        };
        let tier = match form.tier.trim() {
            "" => None,
            text => match text.parse::<i64>() {
                Ok(tier) if (0..=TIER_MAX).contains(&tier) => Some(tier),
                _ => return Ok(SheetSaved::BadTier),
            },
        };
        let typed = optional(&form.picture_url, URL_MAX);
        if typed.as_deref().is_some_and(|url| !url.starts_with("https://")) {
            return Ok(SheetSaved::BadPicture);
        }
        let (race, title) = (optional(&form.race, NAME_MAX), optional(&form.title, NAME_MAX));
        let (bio, notes) = (optional(&form.bio, TEXT_MAX), optional(&form.notes, TEXT_MAX));
        let at = self.clock.now();

        self.store.write(|tx| {
            let before = match target {
                Target::Existing(id) => match repo::get(tx, id)? {
                    Some(row) => Some(Box::new(row)),
                    None => return Ok(SheetSaved::NotFound),
                },
                Target::NewNpc => None,
            };
            let id = before.as_ref().map(|row| row.id);
            let player = before.as_ref().map_or(CAST, |row| row.player_id);
            if let Some(prefix) = prefix.as_deref() {
                let owner = repo::by_prefix(tx, player, prefix)?;
                if let Some(owner) = owner.filter(|o| Some(o.id) != id) {
                    return Ok(SheetSaved::PrefixTaken(owner.name));
                }
            }
            let (picture, picture_message) = picture_after(form.picture, typed, before.as_deref());

            let target = match id {
                Some(id) => id,
                None => {
                    let slot = repo::next_slot(tx, CAST)?;
                    let new = NewCharacter { player_id: CAST, slot, name: &name, staff_id, at };
                    repo::insert(tx, &new)?.expect("a fresh cast slot")
                }
            };
            let fields = SheetFields {
                name: &name,
                prefix: prefix.as_deref(),
                race: race.as_deref(),
                tier,
                title: title.as_deref(),
                bio: bio.as_deref(),
                picture_url: picture.as_deref(),
                picture_message,
                notes: notes.as_deref(),
                posting: form.posting.on(),
            };
            repo::save_sheet(tx, target, &fields)?;
            let after = Box::new(repo::get(tx, target)?.expect("the sheet just saved"));
            Ok(SheetSaved::Done { before, after })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::Services;

    const STAFF: u64 = 9;

    #[test]
    fn a_player_sets_their_own_prefix_unique_among_their_characters_only() {
        let services = Services::memory(0);
        let add = |player, slot, name| match services.characters().add(player, slot, name, STAFF).unwrap() {
            crate::services::characters::AddCharacter::Added(row) => row.id,
            other => panic!("{other:?}"),
        };
        let (nova, nicolas, noxa) = (add(157, 1, "Captain Nova"), add(157, 2, "Nicolas"), add(410, 1, "Noxa"));
        let sheets = services.sheets();
        assert!(matches!(sheets.set_posting(157, nova, "N", Posting::ByPrefix).unwrap(), PostingSaved::Done(row) if row.prefix.as_deref() == Some("n")));
        assert_eq!(sheets.set_posting(157, nicolas, "n", Posting::ByPrefix).unwrap(), PostingSaved::PrefixTaken("Captain Nova".to_owned()));
        // Another player may use the same prefix.
        assert!(matches!(sheets.set_posting(410, noxa, "n", Posting::ByPrefix).unwrap(), PostingSaved::Done(_)));
        assert_eq!(sheets.set_posting(410, nova, "x", Posting::ByPrefix).unwrap(), PostingSaved::NotYours);
        assert!(matches!(sheets.set_posting(157, nova, "n", Posting::Off).unwrap(), PostingSaved::Done(row) if !row.posting));
    }

    #[test]
    fn expressions_are_named_linked_and_coloured() {
        let services = Services::memory(0);
        let sheets = services.sheets();
        let yenma = saved(sheets.save(Target::NewNpc, &form("King Yenma", "yen"), STAFF).unwrap());
        let ExpressionSaved::Done(happy) = sheets.save_expression(yenma.id, "Happy:", "https://i.imgur.com/h.png", "#C0392B").unwrap() else {
            panic!("not saved");
        };
        assert_eq!((happy.name.as_str(), happy.color), ("happy", Some(0xC0_39_2B)));
        assert_eq!(sheets.save_expression(yenma.id, "two words", "https://x", "").unwrap(), ExpressionSaved::BadName);
        assert_eq!(sheets.save_expression(yenma.id, "sad", "javascript:x", "").unwrap(), ExpressionSaved::BadLink);
        assert_eq!(sheets.save_expression(yenma.id, "sad", "https://x", "blue").unwrap(), ExpressionSaved::BadColor);
        assert_eq!(sheets.expressions(yenma.id).unwrap().len(), 1);
        assert!(sheets.remove_expression(yenma.id, "HAPPY").unwrap());
        assert!(sheets.expressions(yenma.id).unwrap().is_empty());
        assert_eq!(expression_refused("two words", ""), Some(ExpressionSaved::BadName));
        assert_eq!(expression_refused("sad", "blue"), Some(ExpressionSaved::BadColor));
        assert_eq!(expression_refused("Sad", "#C0392B"), None);
    }

    fn form(name: &str, prefix: &str) -> SheetForm {
        SheetForm { name: name.to_owned(), prefix: prefix.to_owned(), ..SheetForm::default() }
    }

    fn saved(outcome: SheetSaved) -> CharacterRow {
        match outcome {
            SheetSaved::Done { after, .. } => *after,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn npcs_belong_to_the_cast_with_unique_prefixes() {
        let services = Services::memory(0);
        let sheets = services.sheets();
        let yenma = saved(sheets.save(Target::NewNpc, &form("King Yenma", "Yen"), STAFF).unwrap());
        let shi = saved(sheets.save(Target::NewNpc, &form("The Grand Nurse: Shi", "shi"), STAFF).unwrap());
        assert_eq!((yenma.player_id, yenma.slot, yenma.prefix.as_deref()), (CAST, 1, Some("yen")));
        assert_eq!(shi.slot, 2);

        assert_eq!(sheets.save(Target::NewNpc, &form("Copycat", "YEN"), STAFF).unwrap(), SheetSaved::PrefixTaken("King Yenma".to_owned()));
        assert_eq!(sheets.save(Target::NewNpc, &form("Talker", "note"), STAFF).unwrap(), SheetSaved::BadPrefix(BadPrefix::Reserved.reason()));
        assert_eq!(sheets.npcs().unwrap().iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["King Yenma", "The Grand Nurse: Shi"]);
    }

    #[test]
    fn a_save_checks_the_whole_sheet_and_reports_what_changed() {
        let services = Services::memory(0);
        let sheets = services.sheets();
        let yenma = saved(sheets.save(Target::NewNpc, &form("King Yenma", "yen"), STAFF).unwrap());

        let mut edit = form("King Yenma", "yen");
        edit.race = "Ogre".to_owned();
        edit.tier = "12".to_owned();
        edit.notes = "Judges the dead. Never fights.".to_owned();
        let SheetSaved::Done { before, after } = sheets.save(Target::Existing(yenma.id), &edit, STAFF).unwrap() else {
            panic!("not saved");
        };
        assert_eq!(changed(&before.unwrap(), &after), ["race", "tier", "notes"]);

        edit.tier = "a lot".to_owned();
        assert_eq!(sheets.save(Target::Existing(yenma.id), &edit, STAFF).unwrap(), SheetSaved::BadTier);
        edit.tier = String::new();
        edit.picture_url = "javascript:alert(1)".to_owned();
        assert_eq!(sheets.save(Target::Existing(yenma.id), &edit, STAFF).unwrap(), SheetSaved::BadPicture);
    }

    #[test]
    fn an_upload_stays_until_a_link_or_the_icon_replaces_it() {
        let services = Services::memory(0);
        let sheets = services.sheets();
        let yenma = saved(sheets.save(Target::NewNpc, &form("King Yenma", "yen"), STAFF).unwrap());
        let uploaded = sheets.set_picture(yenma.id, "https://cdn.discordapp.com/a.png?ex=1", 77).unwrap().unwrap();
        assert_eq!(uploaded.picture_message, Some(77));

        // A save with the link left blank keeps the upload.
        let kept = saved(sheets.save(Target::Existing(yenma.id), &form("King Yenma", "yen"), STAFF).unwrap());
        assert_eq!((kept.picture_url.as_deref(), kept.picture_message), (Some("https://cdn.discordapp.com/a.png?ex=1"), Some(77)));

        let mut linked = form("King Yenma", "yen");
        linked.picture_url = "https://i.imgur.com/yenma.png".to_owned();
        let linked = saved(sheets.save(Target::Existing(yenma.id), &linked, STAFF).unwrap());
        assert_eq!((linked.picture_url.as_deref(), linked.picture_message), (Some("https://i.imgur.com/yenma.png"), None));

        let mut icon = form("King Yenma", "yen");
        icon.picture = PictureChoice::Icon;
        let icon = saved(sheets.save(Target::Existing(yenma.id), &icon, STAFF).unwrap());
        assert_eq!((icon.picture_url, icon.picture_message), (None, None));
        assert_eq!(sheets.set_picture(999, "https://x", 1).unwrap(), None);
    }

    #[test]
    fn a_players_sheet_is_edited_in_place_and_stays_theirs() {
        let services = Services::memory(0);
        let crate::services::characters::AddCharacter::Added(nova) = services.characters().add(157, 1, "Captain Nova", STAFF).unwrap() else {
            panic!("not added");
        };
        let mut edit = form("Captain Nova", "nova");
        edit.race = "Earthling".to_owned();
        let after = saved(services.sheets().save(Target::Existing(nova.id), &edit, STAFF).unwrap());
        assert_eq!((after.player_id, after.slot, after.race.as_deref()), (157, 1, Some("Earthling")));
        assert!(services.sheets().npcs().unwrap().is_empty());
        assert_eq!(services.sheets().save(Target::Existing(999), &edit, STAFF).unwrap(), SheetSaved::NotFound);
    }
}
