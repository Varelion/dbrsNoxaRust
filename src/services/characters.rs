//! Characters: a player's slots, and the one they play now (/set).

use anyhow::Result;

use crate::domain::dragon_balls::BallSet;
use crate::domain::items::Item;
use crate::store::characters::{self as repo, CharacterRow, NewCharacter};
use crate::store::{Store, belongings, mcguffins};

use super::{Clock, clean_line};

pub(crate) const SLOT_MAX: i64 = 10;
pub(crate) const NAME_MAX: usize = 32;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AddCharacter {
    Added(Box<CharacterRow>),
    SlotTaken,
    BadSlot,
    NoName,
}

/// A character with what they carry: what /profile shows.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Profile {
    pub(crate) character: CharacterRow,
    pub(crate) items: Vec<(Item, i64)>,
    pub(crate) balls: Vec<(BallSet, i64)>,
    /// (emoji, name) of the McGuffins held, in play only.
    pub(crate) mcguffins: Vec<(String, String)>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SetActive {
    Set(Box<CharacterRow>),
    NotTheirs,
}

pub(crate) struct Characters<'a> {
    store: &'a Store,
    clock: &'a Clock,
}

impl<'a> Characters<'a> {
    pub(crate) const fn new(store: &'a Store, clock: &'a Clock) -> Self {
        Self { store, clock }
    }

    pub(crate) fn add(&self, player_id: u64, slot: i64, name: &str, staff_id: u64) -> Result<AddCharacter> {
        let name = clean_line(name, NAME_MAX);
        if name.is_empty() {
            return Ok(AddCharacter::NoName);
        }
        if !(1..=SLOT_MAX).contains(&slot) {
            return Ok(AddCharacter::BadSlot);
        }

        let new = NewCharacter { player_id, slot, name: &name, staff_id, at: self.clock.now() };
        let Some(id) = self.store.write(|tx| repo::insert(tx, &new))? else {
            return Ok(AddCharacter::SlotTaken);
        };
        let added = self.store.read(|c| repo::get(c, id))?.expect("the character just added");
        Ok(AddCharacter::Added(Box::new(added)))
    }

    pub(crate) fn get(&self, id: i64) -> Result<Option<CharacterRow>> {
        self.store.read(|c| repo::get(c, id))
    }

    pub(crate) fn of_player(&self, player_id: u64) -> Result<Vec<CharacterRow>> {
        self.store.read(|c| repo::of_player(c, player_id))
    }

    pub(crate) fn active(&self, player_id: u64) -> Result<Option<CharacterRow>> {
        self.store.read(|c| repo::active(c, player_id))
    }

    /// None when no such character. An item or set this build does not know is left out, not failed on.
    pub(crate) fn profile(&self, character_id: i64) -> Result<Option<Profile>> {
        self.store.read(|c| {
            let Some(character) = repo::get(c, character_id)? else {
                return Ok(None);
            };
            let items = belongings::items(c, character_id)?.into_iter().filter_map(|(key, n)| Some((Item::from_key(&key)?, n))).collect();
            let balls = belongings::balls(c, character_id)?.into_iter().filter_map(|(key, star)| Some((BallSet::from_key(&key)?, star))).collect();
            let mcguffins = mcguffins::held_by(c, character_id)?;
            Ok(Some(Profile { character, items, balls, mcguffins }))
        })
    }

    pub(crate) fn set_active(&self, player_id: u64, character_id: i64) -> Result<SetActive> {
        let Some(row) = self.get(character_id)?.filter(|row| row.player_id == player_id) else {
            return Ok(SetActive::NotTheirs);
        };
        self.store.write(|tx| repo::set_active(tx, player_id, row.id))?;
        Ok(SetActive::Set(Box::new(row)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::Services;

    const ZIBE: u64 = 410;
    const OTHER: u64 = 411;
    const STAFF: u64 = 9;

    fn added(result: AddCharacter) -> CharacterRow {
        match result {
            AddCharacter::Added(row) => *row,
            other => panic!("not added: {other:?}"),
        }
    }

    #[test]
    fn slots_hold_one_character_each_from_one_to_ten() {
        let services = Services::memory(0);
        let characters = services.characters();

        let itami = added(characters.add(ZIBE, 1, "  Itami  ", STAFF).unwrap());
        assert_eq!(itami.name, "Itami");
        assert_eq!(characters.add(ZIBE, 1, "Ustoka", STAFF).unwrap(), AddCharacter::SlotTaken);
        assert_eq!(characters.add(ZIBE, 11, "Ustoka", STAFF).unwrap(), AddCharacter::BadSlot);
        assert_eq!(characters.add(ZIBE, 2, "   ", STAFF).unwrap(), AddCharacter::NoName);
        added(characters.add(ZIBE, 3, "Ustoka", STAFF).unwrap());

        let names: Vec<_> = characters.of_player(ZIBE).unwrap().into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["Itami", "Ustoka"]);
    }

    #[test]
    fn a_profile_shows_what_the_character_carries() {
        let services = Services::memory(0);
        let itami = added(services.characters().add(ZIBE, 1, "Itami", STAFF).unwrap());
        services
            .store_for_tests()
            .write(|tx| {
                tx.execute("INSERT INTO inventory (character_id, item, quantity) VALUES (?1, 'dragon_radar', 1), (?1, 'retired_item', 2)", [itami.id])?;
                tx.execute("INSERT INTO dragon_balls (set_key, star, holder_id) VALUES ('earth', 4, ?1), ('earth', 5, NULL)", [itami.id])?;
                Ok(())
            })
            .unwrap();

        let profile = services.characters().profile(itami.id).unwrap().unwrap();
        assert_eq!(profile.character.picture_url, None);
        assert_eq!(profile.items, [(Item::DragonRadar, 1)]);
        assert_eq!(profile.balls, [(BallSet::Earth, 4)]);
        assert_eq!(services.characters().profile(999).unwrap(), None);
    }

    #[test]
    fn a_player_sets_only_their_own_character_active() {
        let services = Services::memory(0);
        let characters = services.characters();
        let itami = added(characters.add(ZIBE, 1, "Itami", STAFF).unwrap());
        let prika = added(characters.add(OTHER, 1, "Prika", STAFF).unwrap());

        assert_eq!(characters.active(ZIBE).unwrap(), None);
        assert_eq!(characters.set_active(ZIBE, prika.id).unwrap(), SetActive::NotTheirs);
        assert_eq!(characters.set_active(ZIBE, itami.id).unwrap(), SetActive::Set(Box::new(itami.clone())));
        assert_eq!(characters.active(ZIBE).unwrap(), Some(itami));
    }
}
