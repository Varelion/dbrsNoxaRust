//! The shop: staff keep the shelf and anyone's inventory; players buy with Spirit Points. A purchase
//! takes the Spirit Points and adds the item in one transaction, with a ledger line.
//!
//! ```text
//!   buy(ann, 🫘 Senzu Bean, 2)  ─► Bought::Done { cost: 50, after: 12, owned: 2 }
//!   buy(ann, ☁️ Flying Nimbus, 1) with 12 SP ─► Bought::TooPoor { balance: 12, cost: 80 }
//!   set_count(ann, 🫘, 0)        ─► out of her inventory
//! ```

use std::collections::HashMap;

use anyhow::Result;

use crate::domain::shop::{BUY, BUY_MAX, DESCRIPTION_MAX, EMOJI_MAX, NAME_MAX, OWN_MAX, PRICE_MAX, cost, most_owned};
use crate::store::Store;
use crate::store::shop::{self as repo, ItemFields, ItemRow};
use crate::store::spirit::{self as wallet, LedgerEntry};

use super::{Clock, clean_line};

/// Which items a list shows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shelf {
    /// Every item: staff.
    All,
    /// What players may buy.
    Listed,
}

/// Whether players see an item.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Listing {
    Listed,
    Hidden,
}

/// An item as staff typed it.
pub(crate) struct ItemInput<'a> {
    pub(crate) emoji: &'a str,
    pub(crate) name: &'a str,
    pub(crate) description: &'a str,
    pub(crate) price: i64,
    pub(crate) listing: Listing,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Saved {
    Done(ItemRow),
    NoEmoji,
    NoName,
    BadPrice,
    NameTaken,
    Missing,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Bought {
    Done { item: ItemRow, count: i64, cost: i64, after: i64, owned: i64 },
    Missing,
    BadCount,
    TooMany { owned: i64, most: i64 },
    TooPoor { balance: i64, cost: i64 },
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Counted {
    Done { item: ItemRow, before: i64, after: i64 },
    Missing,
    BadCount,
}

pub(crate) struct Shop<'a> {
    store: &'a Store,
    clock: &'a Clock,
}

impl<'a> Shop<'a> {
    pub(crate) const fn new(store: &'a Store, clock: &'a Clock) -> Self {
        Self { store, clock }
    }

    /// Cheapest first.
    pub(crate) fn items(&self, shelf: Shelf) -> Result<Vec<ItemRow>> {
        let all = self.store.read(repo::all)?;
        Ok(all.into_iter().filter(|i| shelf == Shelf::All || i.listed).collect())
    }

    /// How many players own each item.
    pub(crate) fn owners(&self) -> Result<HashMap<i64, usize>> {
        self.store.read(repo::owners)
    }

    pub(crate) fn inventory(&self, player_id: u64) -> Result<Vec<(ItemRow, i64)>> {
        self.store.read(|c| repo::owned(c, player_id))
    }

    pub(crate) fn create(&self, input: &ItemInput) -> Result<Saved> {
        self.save(None, input)
    }

    pub(crate) fn edit(&self, id: i64, input: &ItemInput) -> Result<Saved> {
        self.save(Some(id), input)
    }

    // A new item (None), or one changed; its name unique in any case.
    fn save(&self, id: Option<i64>, input: &ItemInput) -> Result<Saved> {
        let emoji = clean_line(input.emoji, EMOJI_MAX);
        let name = clean_line(input.name, NAME_MAX);
        let description = clean_line(input.description, DESCRIPTION_MAX);
        if emoji.is_empty() {
            return Ok(Saved::NoEmoji);
        }
        if name.is_empty() {
            return Ok(Saved::NoName);
        }
        if !(0..=PRICE_MAX).contains(&input.price) {
            return Ok(Saved::BadPrice);
        }
        let fields = ItemFields { emoji: &emoji, name: &name, description: &description, price: input.price, listed: input.listing == Listing::Listed };

        self.store.write(|tx| {
            let other = repo::named(tx, &name)?.filter(|found| Some(*found) != id);
            if other.is_some() {
                return Ok(Saved::NameTaken);
            }
            let id = match id {
                Some(id) => {
                    if repo::get(tx, id)?.is_none() {
                        return Ok(Saved::Missing);
                    }
                    repo::update(tx, id, &fields)?;
                    id
                }
                None => repo::insert(tx, &fields)?,
            };
            Ok(repo::get(tx, id)?.map_or(Saved::Missing, Saved::Done))
        })
    }

    /// Gone from the shop and every inventory: the item, and how many players lost it.
    pub(crate) fn remove(&self, id: i64) -> Result<Option<(ItemRow, usize)>> {
        self.store.write(|tx| {
            let Some(item) = repo::get(tx, id)? else {
                return Ok(None);
            };
            let owners = repo::owners(tx)?.get(&id).copied().unwrap_or_default();
            repo::delete(tx, id)?;
            Ok(Some((item, owners)))
        })
    }

    /// A player buys `count` of a listed item: the Spirit Points and the item change together.
    pub(crate) fn buy(&self, player_id: u64, id: i64, count: i64) -> Result<Bought> {
        if !(1..=BUY_MAX).contains(&count) {
            return Ok(Bought::BadCount);
        }
        let at = self.clock.now();
        self.store.write(|tx| {
            let Some(item) = repo::get(tx, id)?.filter(|i| i.listed) else {
                return Ok(Bought::Missing);
            };
            let owned = repo::count(tx, player_id, id)?;
            let most = most_owned(item.price);
            if owned + count > most {
                return Ok(Bought::TooMany { owned, most });
            }
            let balance = wallet::balance(tx, player_id)?;
            let Some(cost) = cost(item.price, count).filter(|c| *c <= balance) else {
                return Ok(Bought::TooPoor { balance, cost: cost(item.price, count).unwrap_or(i64::MAX) });
            };

            let after = balance - cost;
            if cost > 0 {
                let memo = format!("Bought {count} × {}", item.label());
                let entry = LedgerEntry { player_id, delta: -cost, balance: after, kind: BUY, memo: &memo, staff_id: player_id, at };
                wallet::record(tx, &entry)?;
            }
            repo::set_count(tx, player_id, id, owned + count)?;
            Ok(Bought::Done { item, count, cost, after, owned: owned + count })
        })
    }

    /// Staff set how many of an item a player owns; 0 takes it away. No Spirit Points move.
    pub(crate) fn set_count(&self, player_id: u64, id: i64, count: i64) -> Result<Counted> {
        if !(0..=OWN_MAX).contains(&count) {
            return Ok(Counted::BadCount);
        }
        self.store.write(|tx| {
            let Some(item) = repo::get(tx, id)? else {
                return Ok(Counted::Missing);
            };
            let before = repo::count(tx, player_id, id)?;
            repo::set_count(tx, player_id, id, count)?;
            Ok(Counted::Done { item, before, after: count })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::spirit::SpAction;
    use crate::services::Services;
    use crate::services::spirit::Change;

    const NOW: i64 = 1_795_000_000_000;
    const ANN: u64 = 42;
    const STAFF: u64 = 9;

    fn named(services: &Services, name: &str) -> ItemRow {
        services.shop().items(Shelf::All).unwrap().into_iter().find(|i| i.name == name).unwrap()
    }

    fn give(services: &Services, amount: i64) {
        services.spirit().change(&Change { action: SpAction::Give, player_id: ANN, amount, memo: "Event", staff_id: STAFF }).unwrap();
    }

    fn input<'a>(emoji: &'a str, name: &'a str, price: i64) -> ItemInput<'a> {
        ItemInput { emoji, name, description: "", price, listing: Listing::Listed }
    }

    #[test]
    fn the_mock_shelf_is_stocked_cheapest_first() {
        let services = Services::memory(NOW);
        let items = services.shop().items(Shelf::Listed).unwrap();
        assert!(items.len() >= 5);
        assert_eq!(items[0].label(), "🎁 Starter Capsule");
        assert!(items.windows(2).all(|w| w[0].price <= w[1].price));
    }

    #[test]
    fn buying_takes_spirit_points_and_fills_the_inventory() {
        let services = Services::memory(NOW);
        let shop = services.shop();
        let senzu = named(&services, "Senzu Bean");
        give(&services, 60);

        let Bought::Done { cost, after, owned, .. } = shop.buy(ANN, senzu.id, 2).unwrap() else {
            panic!("not bought");
        };
        assert_eq!((cost, after, owned), (50, 10, 2));
        assert_eq!(services.spirit().balance(ANN).unwrap(), 10);
        assert_eq!(shop.inventory(ANN).unwrap(), [(senzu.clone(), 2)]);

        let line = &services.spirit().history(ANN, 1).unwrap()[0];
        assert_eq!((line.kind.as_str(), line.delta, line.memo.as_str()), ("buy", -50, "Bought 2 × 🫘 Senzu Bean"));
    }

    #[test]
    fn refused_purchases_change_nothing() {
        let services = Services::memory(NOW);
        let shop = services.shop();
        let senzu = named(&services, "Senzu Bean");
        give(&services, 30);

        assert_eq!(shop.buy(ANN, senzu.id, 2).unwrap(), Bought::TooPoor { balance: 30, cost: 50 });
        assert_eq!(shop.buy(ANN, senzu.id, 0).unwrap(), Bought::BadCount);
        assert_eq!(shop.buy(ANN, senzu.id, BUY_MAX + 1).unwrap(), Bought::BadCount);
        assert_eq!(shop.buy(ANN, 9999, 1).unwrap(), Bought::Missing);
        assert_eq!(services.spirit().balance(ANN).unwrap(), 30);
        assert!(shop.inventory(ANN).unwrap().is_empty());

        shop.edit(senzu.id, &ItemInput { listing: Listing::Hidden, ..input("🫘", "Senzu Bean", 25) }).unwrap();
        assert_eq!(shop.buy(ANN, senzu.id, 1).unwrap(), Bought::Missing);
    }

    #[test]
    fn a_free_item_is_had_once_and_leaves_no_ledger_line() {
        let services = Services::memory(NOW);
        let shop = services.shop();
        let gift = named(&services, "Starter Capsule");

        assert!(matches!(shop.buy(ANN, gift.id, 1).unwrap(), Bought::Done { cost: 0, .. }));
        assert_eq!(shop.buy(ANN, gift.id, 1).unwrap(), Bought::TooMany { owned: 1, most: 1 });
        assert!(services.spirit().history(ANN, 5).unwrap().is_empty());
    }

    #[test]
    fn staff_keep_the_shelf_and_any_inventory() {
        let services = Services::memory(NOW);
        let shop = services.shop();

        let Saved::Done(orb) = shop.create(&input("🔮", "  Crystal   Ball ", 30)).unwrap() else {
            panic!("not created");
        };
        assert_eq!(orb.name, "Crystal Ball");
        assert_eq!(shop.create(&input("🔮", "crystal ball", 5)).unwrap(), Saved::NameTaken);
        assert_eq!(shop.create(&input(" ", "Orb", 5)).unwrap(), Saved::NoEmoji);
        assert_eq!(shop.create(&input("🔮", "Orb", -1)).unwrap(), Saved::BadPrice);
        assert!(matches!(shop.edit(orb.id, &input("🔮", "Crystal Ball", 35)).unwrap(), Saved::Done(ItemRow { price: 35, .. })));

        assert!(matches!(shop.set_count(ANN, orb.id, 3).unwrap(), Counted::Done { before: 0, after: 3, .. }));
        assert_eq!(shop.set_count(ANN, orb.id, OWN_MAX + 1).unwrap(), Counted::BadCount);
        assert_eq!(shop.owners().unwrap().get(&orb.id), Some(&1));

        let (gone, owners) = shop.remove(orb.id).unwrap().unwrap();
        assert_eq!((gone.name.as_str(), owners), ("Crystal Ball", 1));
        assert!(shop.inventory(ANN).unwrap().is_empty());
        assert!(shop.remove(orb.id).unwrap().is_none());
    }
}
