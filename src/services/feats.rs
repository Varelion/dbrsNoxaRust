//! Feats: what a character did, as staff saw it, linked to the posts that show it. Silent: only staff read
//! them, beside a tier-up request. Archive hides one from lists and reviews; restore brings it back.
//! Nothing deletes a feat.

use anyhow::Result;
use rusqlite::Connection;

use crate::domain::feats::{FeatCounts, FeatKind, FeatState, LINKS_MAX, NOTE_MAX, REASON_MAX, feat_code};
use crate::store::feats::{self as repo, FeatFields, FeatRow, LinkRow, NewLink};
use crate::store::{Store, characters};

use super::{Clock, clean_line};

const MENU_LIMIT: usize = 25;
// A typed filter reads this many of the latest feats before matching.
const MENU_SCAN: usize = 625;

/// A post: where a feat's proof is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PostRef {
    pub(crate) channel_id: u64,
    pub(crate) message_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ArchiveMark {
    pub(crate) by: u64,
    pub(crate) at: i64,
    pub(crate) reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FeatView {
    pub(crate) id: i64,
    pub(crate) code: String,
    pub(crate) character_id: i64,
    pub(crate) character_name: String,
    pub(crate) player_id: u64,
    pub(crate) kind: FeatKind,
    pub(crate) counts: FeatCounts,
    pub(crate) note: String,
    pub(crate) added_by: u64,
    pub(crate) edited_by: Option<u64>,
    pub(crate) edited_at: Option<i64>,
    pub(crate) archived: Option<ArchiveMark>,
    pub(crate) links: Vec<LinkRow>,
}

pub(crate) struct NewFeat<'a> {
    pub(crate) character_id: i64,
    pub(crate) kind: FeatKind,
    pub(crate) counts: FeatCounts,
    pub(crate) note: &'a str,
    pub(crate) post: Option<PostRef>,
    pub(crate) staff_id: u64,
}

/// What changes; `None` keeps the field as it is.
pub(crate) struct FeatEdit<'a> {
    pub(crate) id: i64,
    pub(crate) kind: Option<FeatKind>,
    pub(crate) counts: Option<FeatCounts>,
    pub(crate) note: Option<&'a str>,
    pub(crate) staff_id: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Added {
    Done(Box<FeatView>),
    NoNote,
    NoCharacter,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Edited {
    Done { before: Box<FeatView>, after: Box<FeatView> },
    NotFound,
    Archived,
    NoNote,
    NoChange,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Linked {
    Done(Box<FeatView>),
    NotFound,
    Archived,
    Known,
    Full,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Unlinked {
    Done { feat: Box<FeatView>, removed: PostRef },
    NotFound,
    NoLink,
}

/// Archive and restore: done, no such feat, or it is that way already.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Moved {
    Done(Box<FeatView>),
    NotFound,
    Already,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MenuItem {
    pub(crate) id: i64,
    pub(crate) code: String,
    pub(crate) character_name: String,
    pub(crate) kind: FeatKind,
    pub(crate) note: String,
}

// A row as the Discord layer reads it, with its links. An unknown key (a newer kind read by an older
// build) reads as a note rather than failing the list.
fn view_of(conn: &Connection, row: FeatRow) -> rusqlite::Result<FeatView> {
    let links = repo::links(conn, row.id)?;
    let archived = row.archived_at.map(|at| ArchiveMark { by: row.archived_by.unwrap_or_default(), at, reason: row.archive_reason });
    Ok(FeatView {
        id: row.id,
        code: feat_code(row.id),
        character_id: row.character_id,
        character_name: row.character_name,
        player_id: row.player_id,
        kind: FeatKind::from_key(&row.kind).unwrap_or(FeatKind::Note),
        counts: FeatCounts::from_key(&row.counts).unwrap_or(FeatCounts::Nothing),
        note: row.note,
        added_by: row.added_by,
        edited_by: row.edited_by,
        edited_at: row.edited_at,
        archived,
        links,
    })
}

fn view_by_id(conn: &Connection, id: i64) -> rusqlite::Result<Option<FeatView>> {
    repo::get(conn, id)?.map(|row| view_of(conn, row)).transpose()
}

fn note_of(text: &str) -> String {
    text.trim().chars().take(NOTE_MAX).collect()
}

pub(crate) struct Feats<'a> {
    store: &'a Store,
    clock: &'a Clock,
}

impl<'a> Feats<'a> {
    pub(crate) const fn new(store: &'a Store, clock: &'a Clock) -> Self {
        Self { store, clock }
    }

    pub(crate) fn get(&self, id: i64) -> Result<Option<FeatView>> {
        self.store.read(|c| view_by_id(c, id))
    }

    pub(crate) fn list(&self, character_id: i64, state: FeatState) -> Result<Vec<FeatView>> {
        self.store.read(|c| repo::of_character(c, character_id, state)?.into_iter().map(|row| view_of(c, row)).collect())
    }

    pub(crate) fn add(&self, new: &NewFeat) -> Result<Added> {
        let note = note_of(new.note);
        if note.is_empty() {
            return Ok(Added::NoNote);
        }

        let at = self.clock.now();
        self.store.write(|tx| {
            if characters::get(tx, new.character_id)?.is_none() {
                return Ok(Added::NoCharacter);
            }
            let fields = repo::NewFeat {
                character_id: new.character_id,
                kind: new.kind.key(),
                counts: new.counts.key(),
                note: &note,
                staff_id: new.staff_id,
                at,
            };
            let id = repo::insert(tx, &fields)?;
            if let Some(post) = new.post {
                let link = NewLink { feat_id: id, channel_id: post.channel_id, message_id: post.message_id, staff_id: new.staff_id, at };
                repo::add_link(tx, &link)?;
            }
            Ok(Added::Done(Box::new(view_by_id(tx, id)?.expect("the feat just added"))))
        })
    }

    pub(crate) fn edit(&self, edit: &FeatEdit) -> Result<Edited> {
        let at = self.clock.now();
        self.store.write(|tx| {
            let Some(before) = view_by_id(tx, edit.id)? else {
                return Ok(Edited::NotFound);
            };
            if before.archived.is_some() {
                return Ok(Edited::Archived);
            }
            let note = edit.note.map(note_of);
            if note.as_deref() == Some("") {
                return Ok(Edited::NoNote);
            }

            let kind = edit.kind.unwrap_or(before.kind);
            let counts = edit.counts.unwrap_or(before.counts);
            let note = note.unwrap_or_else(|| before.note.clone());
            if kind == before.kind && counts == before.counts && note == before.note {
                return Ok(Edited::NoChange);
            }

            let fields = FeatFields { kind: kind.key(), counts: counts.key(), note: &note, staff_id: edit.staff_id, at };
            repo::update(tx, edit.id, &fields)?;
            let after = view_by_id(tx, edit.id)?.expect("the feat just edited");
            Ok(Edited::Done { before: Box::new(before), after: Box::new(after) })
        })
    }

    pub(crate) fn link(&self, id: i64, post: PostRef, staff_id: u64) -> Result<Linked> {
        let at = self.clock.now();
        self.store.write(|tx| {
            let Some(feat) = view_by_id(tx, id)? else {
                return Ok(Linked::NotFound);
            };
            if feat.archived.is_some() {
                return Ok(Linked::Archived);
            }
            if feat.links.len() >= LINKS_MAX {
                return Ok(Linked::Full);
            }

            let link = NewLink { feat_id: id, channel_id: post.channel_id, message_id: post.message_id, staff_id, at };
            if !repo::add_link(tx, &link)? {
                return Ok(Linked::Known);
            }
            Ok(Linked::Done(Box::new(view_by_id(tx, id)?.expect("the feat just linked"))))
        })
    }

    /// `removed` names the post, so the log can show what went.
    pub(crate) fn unlink(&self, id: i64, link_id: i64) -> Result<Unlinked> {
        self.store.write(|tx| {
            let Some(feat) = view_by_id(tx, id)? else {
                return Ok(Unlinked::NotFound);
            };
            let Some(link) = feat.links.iter().find(|l| l.id == link_id) else {
                return Ok(Unlinked::NoLink);
            };
            let removed = PostRef { channel_id: link.channel_id, message_id: link.message_id };
            repo::remove_link(tx, id, link_id)?;
            Ok(Unlinked::Done { feat: Box::new(view_by_id(tx, id)?.expect("the feat just unlinked")), removed })
        })
    }

    pub(crate) fn archive(&self, id: i64, reason: Option<&str>, staff_id: u64) -> Result<Moved> {
        let at = self.clock.now();
        let reason = reason.map(|r| clean_line(r, REASON_MAX)).filter(|r| !r.is_empty());
        self.store.write(|tx| {
            let Some(feat) = view_by_id(tx, id)? else {
                return Ok(Moved::NotFound);
            };
            if feat.archived.is_some() {
                return Ok(Moved::Already);
            }
            repo::archive(tx, id, staff_id, at, reason.as_deref())?;
            Ok(Moved::Done(Box::new(view_by_id(tx, id)?.expect("the feat just archived"))))
        })
    }

    pub(crate) fn restore(&self, id: i64, staff_id: u64) -> Result<Moved> {
        let at = self.clock.now();
        self.store.write(|tx| {
            let Some(feat) = view_by_id(tx, id)? else {
                return Ok(Moved::NotFound);
            };
            if feat.archived.is_none() {
                return Ok(Moved::Already);
            }
            repo::restore(tx, id, staff_id, at)?;
            Ok(Moved::Done(Box::new(view_by_id(tx, id)?.expect("the feat just restored"))))
        })
    }

    /// A staff menu: feats in that state whose code, character or note holds the typed text, newest first.
    pub(crate) fn menu(&self, state: FeatState, typed: &str) -> Result<Vec<MenuItem>> {
        let words = typed.trim().to_lowercase();
        let scan = if words.is_empty() { MENU_LIMIT } else { MENU_SCAN };
        let rows = self.store.read(|c| repo::latest(c, state, scan))?;

        let items = rows
            .into_iter()
            .map(|row| MenuItem {
                id: row.id,
                code: feat_code(row.id),
                character_name: row.character_name,
                kind: FeatKind::from_key(&row.kind).unwrap_or(FeatKind::Note),
                note: row.note,
            })
            .filter(|item| words.is_empty() || format!("{} {} {}", item.code, item.character_name, item.note).to_lowercase().contains(&words))
            .take(MENU_LIMIT)
            .collect();
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::Services;
    use crate::services::characters::AddCharacter;

    const NOW: i64 = 1_795_000_000_000;
    const LATER: i64 = NOW + 60_000;
    const ZIBE: u64 = 410;
    const STAFF: u64 = 9;
    const OTHER_STAFF: u64 = 10;
    const COLOSSEUM: PostRef = PostRef { channel_id: 700, message_id: 7001 };
    const BADLANDS: PostRef = PostRef { channel_id: 701, message_id: 7011 };

    fn itami(services: &Services) -> i64 {
        match services.characters().add(ZIBE, 1, "Itami", STAFF).unwrap() {
            AddCharacter::Added(row) => row.id,
            other => panic!("{other:?}"),
        }
    }

    fn victory(services: &Services, character_id: i64) -> FeatView {
        let new = NewFeat {
            character_id,
            kind: FeatKind::Victory,
            counts: FeatCounts::Fight,
            note: "Beat Kewa; Kewa yielded.",
            post: Some(COLOSSEUM),
            staff_id: STAFF,
        };
        match services.feats().add(&new).unwrap() {
            Added::Done(feat) => *feat,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_feat_is_added_with_its_post_and_lists_under_its_character() {
        let services = Services::memory(NOW);
        let character = itami(&services);
        let feat = victory(&services, character);

        assert_eq!(feat.code, "F-0001");
        assert_eq!(feat.character_name, "Itami");
        assert_eq!(feat.player_id, ZIBE);
        assert_eq!(feat.archived, None);
        assert_eq!(feat.links.iter().map(|l| (l.channel_id, l.message_id)).collect::<Vec<_>>(), [(700, 7001)]);
        assert_eq!(services.feats().list(character, FeatState::Active).unwrap().len(), 1);
    }

    #[test]
    fn a_feat_needs_a_note_and_a_real_character() {
        let services = Services::memory(NOW);
        let character = itami(&services);
        let blank = NewFeat { character_id: character, kind: FeatKind::Plot, counts: FeatCounts::Nothing, note: "  ", post: None, staff_id: STAFF };
        assert_eq!(services.feats().add(&blank).unwrap(), Added::NoNote);
        let ghost = NewFeat { character_id: 999, note: "x", ..blank };
        assert_eq!(services.feats().add(&ghost).unwrap(), Added::NoCharacter);
    }

    #[test]
    fn edit_changes_only_what_is_given_and_records_who() {
        let services = Services::memory(NOW);
        let feat = victory(&services, itami(&services));
        services.clock().set(LATER);

        let edit = FeatEdit { id: feat.id, kind: Some(FeatKind::Defeat), counts: None, note: None, staff_id: OTHER_STAFF };
        let Edited::Done { before, after } = services.feats().edit(&edit).unwrap() else {
            panic!("not edited");
        };
        assert_eq!(before.kind, FeatKind::Victory);
        assert_eq!((after.kind, after.counts), (FeatKind::Defeat, FeatCounts::Fight));
        assert_eq!(after.note, "Beat Kewa; Kewa yielded.");
        assert_eq!((after.edited_by, after.edited_at), (Some(OTHER_STAFF), Some(LATER)));

        assert_eq!(services.feats().edit(&edit).unwrap(), Edited::NoChange);
        assert_eq!(services.feats().edit(&FeatEdit { id: 999, ..edit }).unwrap(), Edited::NotFound);
    }

    #[test]
    fn links_hold_a_post_once_up_to_the_limit() {
        let services = Services::memory(NOW);
        let feat = victory(&services, itami(&services));
        let feats = services.feats();

        assert!(matches!(feats.link(feat.id, BADLANDS, STAFF).unwrap(), Linked::Done(_)));
        assert_eq!(feats.link(feat.id, BADLANDS, STAFF).unwrap(), Linked::Known);

        let second = feats.get(feat.id).unwrap().unwrap().links[1].id;
        let Unlinked::Done { removed, .. } = feats.unlink(feat.id, second).unwrap() else {
            panic!("not unlinked");
        };
        assert_eq!(removed, BADLANDS);
        assert_eq!(feats.unlink(feat.id, second).unwrap(), Unlinked::NoLink);

        for n in feats.get(feat.id).unwrap().unwrap().links.len()..LINKS_MAX {
            feats.link(feat.id, PostRef { channel_id: 702, message_id: 8000 + n as u64 }, STAFF).unwrap();
        }
        assert_eq!(feats.link(feat.id, PostRef { channel_id: 702, message_id: 9999 }, STAFF).unwrap(), Linked::Full);
    }

    #[test]
    fn archive_hides_restore_brings_it_back_whole() {
        let services = Services::memory(NOW);
        let character = itami(&services);
        let feat = victory(&services, character);
        let feats = services.feats();
        services.clock().set(LATER);

        let Moved::Done(archived) = feats.archive(feat.id, Some("Wrong  character"), OTHER_STAFF).unwrap() else {
            panic!("not archived");
        };
        assert_eq!(archived.archived, Some(ArchiveMark { by: OTHER_STAFF, at: LATER, reason: Some("Wrong character".to_owned()) }));
        assert!(feats.list(character, FeatState::Active).unwrap().is_empty());
        assert!(feats.menu(FeatState::Active, "").unwrap().is_empty());
        assert_eq!(feats.list(character, FeatState::Archived).unwrap().len(), 1);

        // Read-only until restored.
        let edit = FeatEdit { id: feat.id, kind: None, counts: None, note: Some("x"), staff_id: STAFF };
        assert_eq!(feats.edit(&edit).unwrap(), Edited::Archived);
        assert_eq!(feats.link(feat.id, BADLANDS, STAFF).unwrap(), Linked::Archived);
        assert_eq!(feats.archive(feat.id, None, STAFF).unwrap(), Moved::Already);

        let Moved::Done(restored) = feats.restore(feat.id, STAFF).unwrap() else {
            panic!("not restored");
        };
        assert_eq!(restored.archived, None);
        assert_eq!(restored.note, feat.note);
        assert_eq!(restored.links, feat.links);
        assert_eq!(restored.edited_by, Some(STAFF));
        assert_eq!(feats.restore(feat.id, STAFF).unwrap(), Moved::Already);
    }

    #[test]
    fn the_menu_matches_code_character_or_note_newest_first() {
        let services = Services::memory(NOW);
        let character = itami(&services);
        victory(&services, character);
        let injury = NewFeat { character_id: character, kind: FeatKind::Injury, counts: FeatCounts::Nothing, note: "Left horn broken.", post: Some(BADLANDS), staff_id: STAFF };
        services.feats().add(&injury).unwrap();

        let codes = |typed: &str| services.feats().menu(FeatState::Active, typed).unwrap().into_iter().map(|i| i.code).collect::<Vec<_>>();
        assert_eq!(codes(""), ["F-0002", "F-0001"]);
        assert_eq!(codes("horn"), ["F-0002"]);
        assert_eq!(codes("f-0001"), ["F-0001"]);
        assert_eq!(codes("itami"), ["F-0002", "F-0001"]);
    }
}
