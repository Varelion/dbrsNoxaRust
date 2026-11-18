//! Worlds: the categories staff mark as worlds. A world's text channels are the roleplay channels,
//! where a McGuffin placed at random may land.
//!
//! ```text
//!   set([Earth, Namek])  ─► the worlds, exactly these; the ones before returned for the log
//!   roleplay(worlds, the server's channels now) ─► every text channel inside a world
//! ```

use std::collections::HashSet;

use anyhow::Result;

use crate::store::{Store, worlds as repo};

use super::guild::{Channel, ChannelKind};

pub(crate) struct Worlds<'a> {
    store: &'a Store,
}

impl<'a> Worlds<'a> {
    pub(crate) const fn new(store: &'a Store) -> Self {
        Self { store }
    }

    pub(crate) fn all(&self) -> Result<Vec<u64>> {
        self.store.read(repo::all)
    }

    /// The worlds become exactly these categories; the ones before are returned for the log.
    pub(crate) fn set(&self, categories: &[u64]) -> Result<Vec<u64>> {
        let before = self.all()?;
        self.store.write(|tx| repo::replace(tx, categories))?;
        Ok(before)
    }
}

/// The roleplay channels: every text channel inside a world, as the server has them now.
pub(crate) fn roleplay(worlds: &[u64], channels: &[Channel]) -> Vec<u64> {
    let worlds: HashSet<u64> = worlds.iter().copied().collect();
    channels.iter().filter(|c| c.kind == ChannelKind::Text && c.parent.is_some_and(|p| worlds.contains(&p))).map(|c| c.id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::Services;

    #[test]
    fn worlds_are_replaced_whole_and_their_text_channels_are_the_roleplay_ones() {
        let services = Services::memory(0);
        assert!(services.worlds().set(&[10, 20]).unwrap().is_empty());
        assert_eq!(services.worlds().set(&[10]).unwrap(), [10, 20]);
        assert_eq!(services.worlds().all().unwrap(), [10]);

        let channel = |id, kind, parent| Channel { id, name: String::new(), kind, parent, position: 0 };
        let server = [
            channel(10, ChannelKind::Category, None),
            channel(11, ChannelKind::Text, Some(10)),
            channel(12, ChannelKind::Other, Some(10)),
            channel(21, ChannelKind::Text, Some(20)),
            channel(30, ChannelKind::Text, None),
        ];
        assert_eq!(roleplay(&[10], &server), [11]);
    }
}
