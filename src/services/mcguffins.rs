//! McGuffins for staff: create, edit, place, give, take back, retire, restore. A one-holder McGuffin
//! given to someone moves to them, and leaves where it lay. Nothing deletes one.
//!
//! ```text
//!   place(id, World(Earth), channels now, worlds)   ─► lies somewhere in Earth (a world, a category)
//!   place(id, Channel(#ruins), …)                    ─► lies in #ruins (any text channel)
//!   place(id, Random, …, pick)                       ─► lies in a channel rolled from every roleplay one
//!   place(id, Nowhere, …)                            ─► off the map
//! ```

use anyhow::Result;

use crate::domain::mcguffins::{DEFAULT_EMOJI, DESCRIPTION_MAX, EMOJI_MAX, Holding, NAME_MAX, Place};
use crate::store::mcguffins::{self as repo, HolderRow, McguffinFields};
use crate::store::{Store, characters};

use super::guild::{Channel, ChannelKind};
use super::worlds::roleplay;
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
    /// Where it lies while nobody carries it; None: nowhere.
    pub(crate) place: Option<Place>,
}

/// Where staff want a McGuffin, as a form asks it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Placing {
    Nowhere,
    World(u64),
    Channel(u64),
    /// Any roleplay channel, rolled.
    Random,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Placed {
    /// `pool`: how many roleplay channels a roll picked from; 0 for any other place.
    Done { before: Option<Place>, after: Option<Place>, pool: usize },
    NotFound,
    /// Not a category staff marked as a world, or one the server no longer has.
    NotAWorld,
    /// Not a text channel the server has now.
    NotAChannel,
    /// No world has a text channel to roll from.
    NoRoleplay,
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
    /// `moved_from`: who held a one-holder McGuffin before; `left`: where it lay, now that it is carried.
    Done { moved_from: Option<String>, left: Option<Place> },
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
        place: row.place.and_then(|(kind, id)| Place::from_parts(&kind, id)),
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

            // One holder at a time: whoever has it gives it up, and it no longer lies anywhere.
            let mut moved_from = None;
            let mut left = None;
            if mcguffin.holding == Holding::One {
                for (holder, name, _) in &mcguffin.holders {
                    repo::take(tx, id, *holder)?;
                    moved_from = Some(name.clone());
                }
                left = mcguffin.place;
                repo::set_place(tx, id, None)?;
            }
            repo::give(tx, id, character_id, staff_id, at)?;
            Ok(Given::Done { moved_from, left })
        })
    }

    /// Where it lies now, checked against the server's channels as they are: a world must be a category
    /// marked as one, a channel a text channel, a roll one of the roleplay channels.
    pub(crate) fn place(&self, id: i64, placing: Placing, channels: &[Channel], worlds: &[u64], pick: impl FnOnce(usize) -> usize) -> Result<Placed> {
        let has = |wanted: u64, kind: ChannelKind| channels.iter().any(|c| c.id == wanted && c.kind == kind);
        let (after, pool) = match placing {
            Placing::Nowhere => (None, 0),
            Placing::World(category) if worlds.contains(&category) && has(category, ChannelKind::Category) => (Some(Place::World(category)), 0),
            Placing::World(_) => return Ok(Placed::NotAWorld),
            Placing::Channel(channel) if has(channel, ChannelKind::Text) => (Some(Place::Channel(channel)), 0),
            Placing::Channel(_) => return Ok(Placed::NotAChannel),
            Placing::Random => {
                let pool = roleplay(worlds, channels);
                let Some(rolled) = pool.get(pick(pool.len())).copied() else {
                    return Ok(Placed::NoRoleplay);
                };
                (Some(Place::Random(rolled)), pool.len())
            }
        };
        self.store.write(|tx| {
            let Some(mcguffin) = view_of(tx, id)? else {
                return Ok(Placed::NotFound);
            };
            repo::set_place(tx, id, after.map(|p| (p.key(), p.id())))?;
            Ok(Placed::Done { before: mcguffin.place, after, pool })
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

        assert_eq!(mg.give(key.id, nova, STAFF).unwrap(), Given::Done { moved_from: None, left: None });
        assert_eq!(mg.give(key.id, nova, STAFF).unwrap(), Given::AlreadyHeld);
        assert_eq!(mg.give(key.id, retsu, STAFF).unwrap(), Given::Done { moved_from: Some("Captain Nova".to_owned()), left: None });
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

    // A server with two worlds' categories (Earth with two text channels, Namek with one), an OOC
    // category, and a channel with no category.
    fn server() -> Vec<Channel> {
        let channel = |id, kind, parent| Channel { id, name: String::new(), kind, parent, position: 0 };
        vec![
            channel(10, ChannelKind::Category, None),
            channel(11, ChannelKind::Text, Some(10)),
            channel(12, ChannelKind::Text, Some(10)),
            channel(20, ChannelKind::Category, None),
            channel(21, ChannelKind::Text, Some(20)),
            channel(30, ChannelKind::Category, None),
            channel(31, ChannelKind::Text, Some(30)),
            channel(40, ChannelKind::Text, None),
        ]
    }

    #[test]
    fn a_mcguffin_lies_in_a_world_a_channel_or_a_rolled_roleplay_channel_checked_against_the_server_now() {
        let services = Services::memory(0);
        let key = made(&services, "Star Key", Holding::One);
        let (mg, channels, worlds) = (services.mcguffins(), server(), [10, 20]);
        let none = |_| 0;

        assert_eq!(mg.place(key.id, Placing::World(10), &channels, &worlds, none).unwrap(), Placed::Done { before: None, after: Some(Place::World(10)), pool: 0 });
        assert_eq!(mg.place(key.id, Placing::World(30), &channels, &worlds, none).unwrap(), Placed::NotAWorld);
        assert_eq!(mg.place(key.id, Placing::World(99), &channels, &[99], none).unwrap(), Placed::NotAWorld);
        assert_eq!(mg.place(key.id, Placing::Channel(10), &channels, &worlds, none).unwrap(), Placed::NotAChannel);
        assert_eq!(
            mg.place(key.id, Placing::Channel(40), &channels, &worlds, none).unwrap(),
            Placed::Done { before: Some(Place::World(10)), after: Some(Place::Channel(40)), pool: 0 }
        );

        // A roll draws from the worlds' text channels only: 11, 12, 21; never OOC's 31 or the loose 40.
        let Placed::Done { after: Some(Place::Random(rolled)), pool, .. } = mg.place(key.id, Placing::Random, &channels, &worlds, |n| n - 1).unwrap() else {
            panic!("not rolled");
        };
        assert_eq!((rolled, pool), (21, 3));
        assert_eq!(mg.place(key.id, Placing::Random, &channels, &[30_000], none).unwrap(), Placed::NoRoleplay);
        assert_eq!(mg.place(999, Placing::Nowhere, &channels, &worlds, none).unwrap(), Placed::NotFound);
        assert_eq!(mg.get(key.id).unwrap().unwrap().place, Some(Place::Random(21)));
    }

    #[test]
    fn a_one_holder_mcguffin_given_leaves_its_place_and_a_shared_one_stays() {
        let services = Services::memory(0);
        let nova = character(&services, 157, "Captain Nova");
        let key = made(&services, "Star Key", Holding::One);
        let map = made(&services, "Treasure Map", Holding::Many);
        let (mg, channels) = (services.mcguffins(), server());
        mg.place(key.id, Placing::Channel(11), &channels, &[10], |_| 0).unwrap();
        mg.place(map.id, Placing::World(10), &channels, &[10], |_| 0).unwrap();

        assert_eq!(mg.give(key.id, nova, STAFF).unwrap(), Given::Done { moved_from: None, left: Some(Place::Channel(11)) });
        assert_eq!(mg.get(key.id).unwrap().unwrap().place, None);
        assert_eq!(mg.give(map.id, nova, STAFF).unwrap(), Given::Done { moved_from: None, left: None });
        assert_eq!(mg.get(map.id).unwrap().unwrap().place, Some(Place::World(10)));
    }
}
