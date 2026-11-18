//! Dragon Ball sets for staff: where each can be searched for, the roll it needs, and where its balls hide.
//!
//! ```text
//!   view(Earth)          ─► areas, need, seven balls (held, hidden, or not placed)
//!   set_areas(Earth, …)  ─► the categories and channels searches count in
//!   set_need(Earth, 70)  ─► the 1d100 a search must reach
//!   place(Earth, 4, #c)  ─► one ball hidden; a held ball stays with its holder
//!   scatter(Earth, …)    ─► every ball nobody holds hidden again, spread over the areas
//! ```

use std::collections::HashSet;
use std::hash::{BuildHasher as _, Hasher as _};

use anyhow::Result;

use crate::domain::dragon_balls::{AreaKind, BallSet, ROLL_MAX, ROLL_MIN, STARS, scatter_plan};
use crate::store::balls::{self as repo, BallRow};
use crate::store::Store;

use super::guild::{Channel, ChannelKind};

/// A hiding place, picked fresh for each ball. Rust's hasher keys are random per process and change
/// on every build_hasher, which is enough randomness for hiding Dragon Balls.
pub(crate) fn random_below(n: usize) -> usize {
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_usize(n);
    usize::try_from(hasher.finish() % n.max(1) as u64).unwrap_or(0)
}

/// The text channels searches count in: the area channels, and every text channel in an area category.
pub(crate) fn searchable(areas: &[Area], channels: &[Channel]) -> Vec<u64> {
    let categories: HashSet<u64> = areas.iter().filter(|a| a.kind == AreaKind::Category).map(|a| a.channel_id).collect();
    let picked: HashSet<u64> = areas.iter().filter(|a| a.kind == AreaKind::Channel).map(|a| a.channel_id).collect();
    channels
        .iter()
        .filter(|c| c.kind == ChannelKind::Text)
        .filter(|c| picked.contains(&c.id) || c.parent.is_some_and(|p| categories.contains(&p)))
        .map(|c| c.id)
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Area {
    pub(crate) channel_id: u64,
    pub(crate) kind: AreaKind,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SetView {
    pub(crate) set: BallSet,
    pub(crate) need: i64,
    pub(crate) areas: Vec<Area>,
    pub(crate) balls: Vec<BallRow>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NeedSet {
    Done { before: i64, after: i64 },
    OutOfRange,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Placed {
    Done,
    Held { name: String },
    BadStar,
}

pub(crate) struct Balls<'a> {
    store: &'a Store,
}

impl<'a> Balls<'a> {
    pub(crate) const fn new(store: &'a Store) -> Self {
        Self { store }
    }

    pub(crate) fn view(&self, set: BallSet) -> Result<SetView> {
        self.store.write(|tx| {
            repo::ensure_set(tx, set.key())?;
            let need = repo::need(tx, set.key())?.unwrap_or(set.default_need());
            let areas = repo::areas(tx, set.key())?
                .into_iter()
                .filter_map(|(channel_id, kind)| Some(Area { channel_id, kind: AreaKind::from_key(&kind)? }))
                .collect();
            Ok(SetView { set, need, areas, balls: repo::balls(tx, set.key())? })
        })
    }

    /// The areas become exactly these; the ones before are returned for the log.
    pub(crate) fn set_areas(&self, set: BallSet, areas: &[Area]) -> Result<Vec<Area>> {
        let before = self.view(set)?.areas;
        let rows: Vec<_> = areas.iter().map(|a| (a.channel_id, a.kind.key())).collect();
        self.store.write(|tx| repo::replace_areas(tx, set.key(), &rows))?;
        Ok(before)
    }

    pub(crate) fn set_need(&self, set: BallSet, need: i64) -> Result<NeedSet> {
        if !(ROLL_MIN..=ROLL_MAX).contains(&need) {
            return Ok(NeedSet::OutOfRange);
        }
        let before = self.view(set)?.need;
        self.store.write(|tx| repo::set_need(tx, set.key(), need))?;
        Ok(NeedSet::Done { before, after: need })
    }

    pub(crate) fn place(&self, set: BallSet, star: i64, channel_id: u64) -> Result<Placed> {
        if !STARS.contains(&star) {
            return Ok(Placed::BadStar);
        }
        let view = self.view(set)?;
        if let Some((_, name, _)) = view.balls.iter().find(|b| b.star == star).and_then(|b| b.holder.clone()) {
            return Ok(Placed::Held { name });
        }
        self.store.write(|tx| repo::place(tx, set.key(), star, channel_id))?;
        Ok(Placed::Done)
    }

    /// Every ball nobody holds, hidden again over `channels`; (star, channel) for each one moved.
    pub(crate) fn scatter(&self, set: BallSet, channels: &[u64], pick: impl FnMut(usize) -> usize) -> Result<Vec<(i64, u64)>> {
        let loose: Vec<i64> = self.view(set)?.balls.iter().filter(|b| b.holder.is_none()).map(|b| b.star).collect();
        let plan = scatter_plan(&loose, channels, pick);
        self.store.write(|tx| {
            for &(star, channel) in &plan {
                repo::place(tx, set.key(), star, channel)?;
            }
            Ok(())
        })?;
        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::Services;
    use crate::services::characters::AddCharacter;

    const NOVA: u64 = 157;

    fn held_by_nova(services: &Services, star: i64) -> i64 {
        let AddCharacter::Added(row) = services.characters().add(NOVA, 1, "Captain Nova", 9).unwrap() else {
            panic!("not added");
        };
        services.balls().view(BallSet::Earth).unwrap();
        services
            .store_for_tests()
            .write(|tx| tx.execute("UPDATE dragon_balls SET holder_id = ?1 WHERE set_key = 'earth' AND star = ?2", [row.id, star]))
            .unwrap();
        row.id
    }

    #[test]
    fn random_below_stays_below() {
        assert!((0..200).map(|_| random_below(7)).all(|n| n < 7));
    }

    #[test]
    fn searches_count_in_area_channels_and_the_text_channels_of_area_categories() {
        let channel = |id, kind, parent| Channel { id, name: String::new(), kind, parent, position: 0 };
        let server = [
            channel(1, ChannelKind::Category, None),
            channel(2, ChannelKind::Text, Some(1)),
            channel(3, ChannelKind::Other, Some(1)),
            channel(4, ChannelKind::Text, None),
            channel(5, ChannelKind::Text, None),
        ];
        let areas = [Area { channel_id: 1, kind: AreaKind::Category }, Area { channel_id: 4, kind: AreaKind::Channel }];
        assert_eq!(searchable(&areas, &server), [2, 4]);
    }

    #[test]
    fn a_new_set_has_seven_unplaced_balls_and_the_default_roll() {
        let services = Services::memory(0);
        let view = services.balls().view(BallSet::Namek).unwrap();
        assert_eq!(view.need, 80);
        assert_eq!(view.balls.len(), 7);
        assert!(view.balls.iter().all(|b| b.holder.is_none() && b.channel_id.is_none()));
        assert!(view.areas.is_empty());
    }

    #[test]
    fn areas_are_replaced_whole_and_the_old_ones_returned() {
        let services = Services::memory(0);
        let balls = services.balls();
        let west = Area { channel_id: 10, kind: AreaKind::Category };
        let kame = Area { channel_id: 20, kind: AreaKind::Channel };

        assert!(balls.set_areas(BallSet::Earth, &[west, kame]).unwrap().is_empty());
        assert_eq!(balls.set_areas(BallSet::Earth, &[kame]).unwrap(), [west, kame]);
        assert_eq!(balls.view(BallSet::Earth).unwrap().areas, [kame]);
        assert!(balls.view(BallSet::Namek).unwrap().areas.is_empty());
    }

    #[test]
    fn the_roll_stays_between_one_and_a_hundred() {
        let services = Services::memory(0);
        assert_eq!(services.balls().set_need(BallSet::Earth, 75).unwrap(), NeedSet::Done { before: 70, after: 75 });
        assert_eq!(services.balls().set_need(BallSet::Earth, 101).unwrap(), NeedSet::OutOfRange);
        assert_eq!(services.balls().view(BallSet::Earth).unwrap().need, 75);
    }

    #[test]
    fn a_held_ball_stays_with_its_holder_and_scatter_leaves_it_alone() {
        let services = Services::memory(0);
        held_by_nova(&services, 1);
        let balls = services.balls();

        assert_eq!(balls.place(BallSet::Earth, 1, 99).unwrap(), Placed::Held { name: "Captain Nova".to_owned() });
        assert_eq!(balls.place(BallSet::Earth, 4, 99).unwrap(), Placed::Done);
        assert_eq!(balls.place(BallSet::Earth, 8, 99).unwrap(), Placed::BadStar);

        let moved = balls.scatter(BallSet::Earth, &[10, 20, 30, 40, 50, 60], |_| 0).unwrap();
        assert_eq!(moved.iter().map(|&(s, _)| s).collect::<Vec<_>>(), [2, 3, 4, 5, 6, 7]);
        let view = balls.view(BallSet::Earth).unwrap();
        assert_eq!(view.balls[0].channel_id, None);
        assert!(view.balls[1..].iter().all(|b| b.channel_id.is_some()));
    }
}
