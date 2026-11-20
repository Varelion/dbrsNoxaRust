//! Rules over the store. The Discord and web layers call these and nothing below them.
//!
//! ```text
//!   discord ─┐
//!   web ─────┴► Services ─┬─► characters()  slots, the active character
//!                         ├─► players()     everyone with a wallet or a character
//!                         ├─► spirit()      Spirit Points and their ledger
//!                         ├─► balls()       Dragon Ball areas, rolls and hiding places
//!                         ├─► mcguffins()   plot items and their holders
//!                         ├─► sheets()      any character's sheet; new NPCs
//!                         ├─► shop()        the shelf, inventories, purchases
//!                         ├─► links()       one-time web links for staff
//!                         ├─► feats()       silent feats and their links
//!                         └─► settings()    the mod log channel
//!                                  │
//!                                  ▼
//!                                Store
//!
//!   web ─► guild::Guild ─► the Discord layer: names, channels, picture uploads, mod log lines
//! ```

pub(crate) mod authority;
pub(crate) mod balls;
pub(crate) mod characters;
pub(crate) mod feats;
pub(crate) mod guild;
pub(crate) mod links;
pub(crate) mod mcguffins;
pub(crate) mod players;
pub(crate) mod proxy;
pub(crate) mod settings;
pub(crate) mod sheets;
pub(crate) mod shop;
pub(crate) mod spirit;
pub(crate) mod tickets;
pub(crate) mod tutorials;
pub(crate) mod worlds;

use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::store::Store;

/// Milliseconds since the epoch: the system's, or a hand-set one for tests.
pub(crate) enum Clock {
    System,
    #[cfg_attr(not(test), allow(dead_code))]
    Fixed(AtomicI64),
}

impl Clock {
    pub(crate) fn now(&self) -> i64 {
        match self {
            Self::System => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)),
            Self::Fixed(at) => at.load(Ordering::Relaxed),
        }
    }

    #[cfg(test)]
    pub(crate) fn set(&self, at: i64) {
        if let Self::Fixed(fixed) = self {
            fixed.store(at, Ordering::Relaxed);
        }
    }
}

pub(crate) struct Services {
    store: Store,
    clock: Clock,
    links: links::Links,
}

impl Services {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        Ok(Self { store: Store::open(path)?, clock: Clock::System, links: links::Links::default() })
    }

    #[cfg(test)]
    pub(crate) fn memory(at: i64) -> Self {
        Self { store: Store::memory().expect("memory database"), clock: Clock::Fixed(AtomicI64::new(at)), links: links::Links::default() }
    }

    #[cfg(test)]
    pub(crate) const fn clock(&self) -> &Clock {
        &self.clock
    }

    #[cfg(test)]
    pub(crate) const fn store_for_tests(&self) -> &Store {
        &self.store
    }

    pub(crate) const fn balls(&self) -> balls::Balls<'_> {
        balls::Balls::new(&self.store)
    }

    pub(crate) const fn characters(&self) -> characters::Characters<'_> {
        characters::Characters::new(&self.store, &self.clock)
    }

    pub(crate) const fn spirit(&self) -> spirit::Spirit<'_> {
        spirit::Spirit::new(&self.store, &self.clock)
    }

    pub(crate) const fn feats(&self) -> feats::Feats<'_> {
        feats::Feats::new(&self.store, &self.clock)
    }

    pub(crate) const fn mcguffins(&self) -> mcguffins::Mcguffins<'_> {
        mcguffins::Mcguffins::new(&self.store, &self.clock)
    }

    pub(crate) const fn sheets(&self) -> sheets::Sheets<'_> {
        sheets::Sheets::new(&self.store, &self.clock)
    }

    pub(crate) const fn shop(&self) -> shop::Shop<'_> {
        shop::Shop::new(&self.store, &self.clock)
    }

    pub(crate) const fn players(&self) -> players::Players<'_> {
        players::Players::new(&self.store)
    }

    pub(crate) const fn links(&self) -> &links::Links {
        &self.links
    }

    pub(crate) const fn proxy(&self) -> proxy::Proxy<'_> {
        proxy::Proxy::new(&self.store, &self.clock)
    }

    pub(crate) const fn tutorials(&self) -> tutorials::Tutorials<'_> {
        tutorials::Tutorials::new(&self.store, &self.clock)
    }

    pub(crate) const fn worlds(&self) -> worlds::Worlds<'_> {
        worlds::Worlds::new(&self.store)
    }

    pub(crate) const fn settings(&self) -> settings::Settings<'_> {
        settings::Settings::new(&self.store)
    }
}

/// Trims, collapses runs of spaces, and cuts to `max` characters: what a memo or reason keeps.
pub(crate) fn clean_line(text: &str, max: usize) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}
