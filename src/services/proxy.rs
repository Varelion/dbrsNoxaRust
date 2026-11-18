//! Posting as a character: who a prefix stands for, and the record of every part posted, so only its
//! author edits or deletes it.
//!
//! ```text
//!   find(author, "nova", happy, Staff?) ─► the author's own character with that prefix, posting on
//!                                          └─ none, and staff ─► the cast's (moderator NPCs)
//!   record(parts) · post(message) · group(first part) · forget(message)
//!   count(character)                    ─► one more roleplay post
//! ```

use anyhow::Result;

use crate::domain::npcs::CAST;
use crate::store::characters::{self, CharacterRow};
use crate::store::expressions::{self, ExpressionRow};
use crate::store::proxied::{self, ProxiedRow};
use crate::store::Store;

use super::Clock;

/// Whether the author may also post as the cast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cast {
    Allowed,
    Denied,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Found {
    pub(crate) character: CharacterRow,
    /// The expression asked for, if the character has it; an unknown one posts with the main picture.
    pub(crate) expression: Option<ExpressionRow>,
    /// One of the cast, posted by staff.
    pub(crate) npc: bool,
}

pub(crate) struct Proxy<'a> {
    store: &'a Store,
    clock: &'a Clock,
}

impl<'a> Proxy<'a> {
    pub(crate) const fn new(store: &'a Store, clock: &'a Clock) -> Self {
        Self { store, clock }
    }

    pub(crate) fn find(&self, author: u64, prefix: &str, expression: Option<&str>, cast: Cast) -> Result<Option<Found>> {
        self.store.read(|c| {
            let own = characters::by_prefix(c, author, prefix)?.filter(|row| row.posting);
            let npc = match (&own, cast) {
                (None, Cast::Allowed) => characters::by_prefix(c, CAST, prefix)?.filter(|row| row.posting),
                _ => None,
            };
            let (character, is_npc) = match (own, npc) {
                (Some(row), _) => (row, false),
                (None, Some(row)) => (row, true),
                (None, None) => return Ok(None),
            };
            let expression = match expression {
                Some(name) => expressions::get(c, character.id, name)?,
                None => None,
            };
            Ok(Some(Found { character, expression, npc: is_npc }))
        })
    }

    /// Every part of a post, stamped now.
    pub(crate) fn record(&self, parts: &[ProxiedRow]) -> Result<()> {
        let at = self.clock.now();
        self.store.write(|tx| parts.iter().try_for_each(|part| proxied::record(tx, &ProxiedRow { at, ..part.clone() })))
    }

    pub(crate) fn post(&self, message_id: u64) -> Result<Option<ProxiedRow>> {
        self.store.read(|c| proxied::get(c, message_id))
    }

    pub(crate) fn group(&self, group_id: u64) -> Result<Vec<ProxiedRow>> {
        self.store.read(|c| proxied::group(c, group_id))
    }

    pub(crate) fn forget(&self, message_id: u64) -> Result<()> {
        self.store.write(|tx| proxied::forget(tx, message_id))
    }

    pub(crate) fn count(&self, character_id: i64) -> Result<()> {
        self.store.write(|tx| characters::count_post(tx, character_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::Services;
    use crate::services::sheets::{Posting, SheetForm, SheetSaved, Target};

    const NOVA_PLAYER: u64 = 157;
    const STAFF: u64 = 9;

    #[test]
    fn a_prefix_is_the_authors_own_first_then_for_staff_the_casts() {
        let services = Services::memory(0);
        let crate::services::characters::AddCharacter::Added(nova) = services.characters().add(NOVA_PLAYER, 1, "Captain Nova", STAFF).unwrap() else {
            panic!("not added");
        };
        services.sheets().set_posting(NOVA_PLAYER, nova.id, "yen", Posting::ByPrefix).unwrap();
        let SheetSaved::Done { after: yenma, .. } = services.sheets().save(Target::NewNpc, &SheetForm { name: "King Yenma".to_owned(), prefix: "yen".to_owned(), ..SheetForm::default() }, STAFF).unwrap() else {
            panic!("no npc");
        };
        services.sheets().save_expression(yenma.id, "angry", "https://i.imgur.com/a.png", "").unwrap();
        let proxy = services.proxy();

        // Nova's player has "yen" too: theirs wins, even as staff.
        assert_eq!(proxy.find(NOVA_PLAYER, "yen", None, Cast::Allowed).unwrap().map(|f| (f.character.name, f.npc)), Some(("Captain Nova".to_owned(), false)));
        // Staff without their own: the cast's, with its expression.
        let found = proxy.find(STAFF, "yen", Some("angry"), Cast::Allowed).unwrap().unwrap();
        assert!(found.npc);
        assert_eq!(found.expression.map(|e| e.name), Some("angry".to_owned()));
        assert!(proxy.find(STAFF, "yen", Some("sleepy"), Cast::Allowed).unwrap().unwrap().expression.is_none());
        // A player never posts as the cast.
        assert!(proxy.find(410, "yen", None, Cast::Denied).unwrap().is_none());
        // Posting off: the prefix posts nothing.
        services.sheets().set_posting(NOVA_PLAYER, nova.id, "yen", Posting::Off).unwrap();
        assert!(proxy.find(NOVA_PLAYER, "yen", None, Cast::Denied).unwrap().is_none());
    }

    #[test]
    fn a_posts_parts_are_found_by_any_one_and_forgotten_one_by_one() {
        let services = Services::memory(0);
        let part = |message_id, part| ProxiedRow { message_id, channel_id: 5, hook_channel: 5, thread_id: None, webhook_id: 7, author_id: 157, character_id: 1, group_id: 100, part, at: 0 };
        services.proxy().record(&[part(100, 1), part(101, 2)]).unwrap();
        assert_eq!(services.proxy().post(101).unwrap().map(|p| p.group_id), Some(100));
        assert_eq!(services.proxy().group(100).unwrap().len(), 2);
        services.proxy().forget(100).unwrap();
        assert_eq!(services.proxy().group(100).unwrap().len(), 1);
    }
}
