//! McGuffins for staff: create, edit, give, take back, retire, restore. A one-holder McGuffin given to
//! someone new moves to them. Nothing deletes one.

use anyhow::Result;

use crate::domain::mcguffins::{DEFAULT_EMOJI, DESCRIPTION_MAX, EMOJI_MAX, Holding, NAME_MAX};
use crate::store::mcguffins::{self as repo, HolderRow, McguffinFields};
use crate::store::{Store, characters};

use super::{Clock, clean_line};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct McguffinView {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) emoji: String,
    pub(crate) description: String,
    pub(crate) holding: Holding,
    pub(crate) retired: bool,
    pub(crate) holders: Vec<HolderRow>,
}

/// What staff typed in the form.
pub(crate) struct Draft<'a> {
    pub(crate) name: &'a str,
    pub(crate) emoji: &'a str,
    pub(crate) description: &'a str,
    pub(crate) holding: Holding,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Saved {
    Done(Box<McguffinView>),
    NoName,
    NameTaken,
    NotFound,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Given {
    /// `moved_from`: the one who held a one-holder McGuffin before.
    Done { moved_from: Option<String> },
    NotFound,
    Retired,
    NoCharacter,
    AlreadyHeld,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Taken {
    Done,
    NotHeld,
}

fn view_of(conn: &rusqlite::Connection, id: i64) -> rusqlite::Result<Option<McguffinView>> {
    let Some(row) = repo::get(conn, id)? else {
        return Ok(None);
    };
    Ok(Some(McguffinView {
        id: row.id,
        name: row.name,
        emoji: row.emoji,
        description: row.description,
        holding: Holding::from_key(&row.holding).unwrap_or(Holding::Many),
        retired: row.retired,
        holders: repo::holders(conn, id)?,
    }))
}

pub(crate) struct Mcguffins<'a> {
    store: &'a Store,
    clock: &'a Clock,
}

impl<'a> Mcguffins<'a> {
    pub(crate) const fn new(store: &'a Store, clock: &'a Clock) -> Self {
        Self { store, clock }
    }

    pub(crate) fn all(&self) -> Result<Vec<McguffinView>> {
        self.store.read(|c| repo::all(c)?.into_iter().filter_map(|row| view_of(c, row.id).transpose()).collect())
    }

    pub(crate) fn get(&self, id: i64) -> Result<Option<McguffinView>> {
        self.store.read(|c| view_of(c, id))
    }

    // Trimmed and cut to size; an empty emoji becomes the default.
    fn fields(draft: &Draft) -> (String, String, String) {
        let name = clean_line(draft.name, NAME_MAX);
        let emoji: String = draft.emoji.trim().chars().take(EMOJI_MAX).collect();
        let emoji = if emoji.is_empty() { DEFAULT_EMOJI.to_owned() } else { emoji };
        (name, emoji, draft.description.trim().chars().take(DESCRIPTION_MAX).collect())
    }

    pub(crate) fn create(&self, draft: &Draft, staff_id: u64) -> Result<Saved> {
        let (name, emoji, description) = Self::fields(draft);
        if name.is_empty() {
            return Ok(Saved::NoName);
        }
        let at = self.clock.now();
        self.store.write(|tx| {
            let fields = McguffinFields { name: &name, emoji: &emoji, description: &description, holding: draft.holding.key() };
            let Some(id) = repo::insert(tx, &fields, staff_id, at)? else {
                return Ok(Saved::NameTaken);
            };
            Ok(Saved::Done(Box::new(view_of(tx, id)?.expect("the McGuffin just made"))))
        })
    }

    pub(crate) fn edit(&self, id: i64, draft: &Draft) -> Result<Saved> {
        let (name, emoji, description) = Self::fields(draft);
        if name.is_empty() {
            return Ok(Saved::NoName);
        }
        self.store.write(|tx| {
            if repo::get(tx, id)?.is_none() {
                return Ok(Saved::NotFound);
            }
            let fields = McguffinFields { name: &name, emoji: &emoji, description: &description, holding: draft.holding.key() };
            if !repo::update(tx, id, &fields)? {
                return Ok(Saved::NameTaken);
            }
            Ok(Saved::Done(Box::new(view_of(tx, id)?.expect("the McGuffin just edited"))))
        })
    }

    pub(crate) fn give(&self, id: i64, character_id: i64, staff_id: u64) -> Result<Given> {
        let at = self.clock.now();
        self.store.write(|tx| {
            let Some(mcguffin) = view_of(tx, id)? else {
                return Ok(Given::NotFound);
            };
            if mcguffin.retired {
                return Ok(Given::Retired);
            }
            if characters::get(tx, character_id)?.is_none() {
                return Ok(Given::NoCharacter);
            }
            if mcguffin.holders.iter().any(|(holder, _, _)| *holder == character_id) {
                return Ok(Given::AlreadyHeld);
            }

            // One holder at a time: whoever has it gives it up.
            let mut moved_from = None;
            if mcguffin.holding == Holding::One {
                for (holder, name, _) in &mcguffin.holders {
                    repo::take(tx, id, *holder)?;
                    moved_from = Some(name.clone());
                }
            }
            repo::give(tx, id, character_id, staff_id, at)?;
            Ok(Given::Done { moved_from })
        })
    }

    pub(crate) fn take(&self, id: i64, character_id: i64) -> Result<Taken> {
        let taken = self.store.write(|tx| repo::take(tx, id, character_id))?;
        Ok(if taken { Taken::Done } else { Taken::NotHeld })
    }

    /// Retired: off every card and out of the give list, holders kept for a restore.
    pub(crate) fn set_retired(&self, id: i64, retired: bool, staff_id: u64) -> Result<Option<McguffinView>> {
        let at = self.clock.now();
        self.store.write(|tx| {
            if repo::get(tx, id)?.is_none() {
                return Ok(None);
            }
            let (by, when) = if retired { (Some(staff_id), Some(at)) } else { (None, None) };
            repo::set_retired(tx, id, by, when)?;
            view_of(tx, id)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::Services;
    use crate::services::characters::AddCharacter;

    const STAFF: u64 = 9;

    fn character(services: &Services, player: u64, name: &str) -> i64 {
        match services.characters().add(player, 1, name, STAFF).unwrap() {
            AddCharacter::Added(row) => row.id,
            other => panic!("{other:?}"),
        }
    }

    fn made(services: &Services, name: &str, holding: Holding) -> McguffinView {
        let draft = Draft { name, emoji: "", description: "  A key to somewhere.  ", holding };
        match services.mcguffins().create(&draft, STAFF).unwrap() {
            Saved::Done(view) => *view,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn names_are_unique_any_case_and_the_emoji_has_a_default() {
        let services = Services::memory(0);
        let key = made(&services, "Star Key", Holding::One);
        assert_eq!((key.emoji.as_str(), key.description.as_str()), ("🔮", "A key to somewhere."));
        let again = Draft { name: "star key", emoji: "🗝️", description: "", holding: Holding::One };
        assert_eq!(services.mcguffins().create(&again, STAFF).unwrap(), Saved::NameTaken);
        assert_eq!(services.mcguffins().create(&Draft { name: "  ", ..again }, STAFF).unwrap(), Saved::NoName);
    }

    #[test]
    fn a_one_holder_mcguffin_moves_and_a_shared_one_spreads() {
        let services = Services::memory(0);
        let (nova, retsu) = (character(&services, 157, "Captain Nova"), character(&services, 160, "Retsu"));
        let key = made(&services, "Star Key", Holding::One);
        let map = made(&services, "Treasure Map", Holding::Many);
        let mg = services.mcguffins();

        assert_eq!(mg.give(key.id, nova, STAFF).unwrap(), Given::Done { moved_from: None });
        assert_eq!(mg.give(key.id, nova, STAFF).unwrap(), Given::AlreadyHeld);
        assert_eq!(mg.give(key.id, retsu, STAFF).unwrap(), Given::Done { moved_from: Some("Captain Nova".to_owned()) });
        assert_eq!(mg.get(key.id).unwrap().unwrap().holders.len(), 1);

        mg.give(map.id, nova, STAFF).unwrap();
        mg.give(map.id, retsu, STAFF).unwrap();
        assert_eq!(mg.get(map.id).unwrap().unwrap().holders.len(), 2);
        assert_eq!(mg.take(map.id, nova).unwrap(), Taken::Done);
        assert_eq!(mg.take(map.id, nova).unwrap(), Taken::NotHeld);
    }

    #[test]
    fn retiring_hides_it_from_cards_and_stops_giving_restore_brings_it_back() {
        let services = Services::memory(0);
        let nova = character(&services, 157, "Captain Nova");
        let key = made(&services, "Star Key", Holding::One);
        let mg = services.mcguffins();
        mg.give(key.id, nova, STAFF).unwrap();

        assert!(mg.set_retired(key.id, true, STAFF).unwrap().unwrap().retired);
        assert!(services.characters().profile(nova).unwrap().unwrap().mcguffins.is_empty());
        assert_eq!(mg.give(key.id, nova, STAFF).unwrap(), Given::Retired);

        assert!(!mg.set_retired(key.id, false, STAFF).unwrap().unwrap().retired);
        assert_eq!(services.characters().profile(nova).unwrap().unwrap().mcguffins, [("🔮".to_owned(), "Star Key".to_owned())]);
    }
}
