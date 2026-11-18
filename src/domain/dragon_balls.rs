//! Dragon Ball sets, seven stars each (the table's CHECK keeps stars 1 to 7), where they can be searched
//! for, and the roll a search needs.

pub(crate) const STARS: [i64; 7] = [1, 2, 3, 4, 5, 6, 7];
pub(crate) const ROLL_MIN: i64 = 1;
pub(crate) const ROLL_MAX: i64 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BallSet {
    Earth,
    Namek,
    BlackStar,
    Spirit,
}

impl BallSet {
    pub(crate) const ALL: [Self; 4] = [Self::Earth, Self::Namek, Self::BlackStar, Self::Spirit];

    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Earth => "earth",
            Self::Namek => "namek",
            Self::BlackStar => "black_star",
            Self::Spirit => "spirit",
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Earth => "Earth",
            Self::Namek => "Namek",
            Self::BlackStar => "Black Star",
            Self::Spirit => "Spirit",
        }
    }

    /// The roll needed until staff set one: Earth 70 and Namek 80 from the old server's FAQ.
    pub(crate) const fn default_need(self) -> i64 {
        match self {
            Self::Earth => 70,
            Self::Namek | Self::BlackStar => 80,
            Self::Spirit => 85,
        }
    }

    pub(crate) fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|set| set.key() == key)
    }
}

/// "Earth 1★".
pub(crate) fn ball_label(set: BallSet, star: i64) -> String {
    format!("{} {star}★", set.label())
}

/// What an area entry covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AreaKind {
    Category,
    Channel,
}

impl AreaKind {
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Category => "category",
            Self::Channel => "channel",
        }
    }

    pub(crate) fn from_key(key: &str) -> Option<Self> {
        [Self::Category, Self::Channel].into_iter().find(|kind| kind.key() == key)
    }
}

/// Where each star goes: a different channel each while there are enough, then they share.
/// `pick(n)` returns an index below n (random in play, fixed in tests).
pub(crate) fn scatter_plan(stars: &[i64], channels: &[u64], mut pick: impl FnMut(usize) -> usize) -> Vec<(i64, u64)> {
    if channels.is_empty() {
        return Vec::new();
    }
    let mut pool = Vec::new();
    stars
        .iter()
        .map(|&star| {
            if pool.is_empty() {
                pool = channels.to_vec();
            }
            let channel = pool.swap_remove(pick(pool.len()) % pool.len());
            (star, channel)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scatter_spreads_stars_over_different_channels_first() {
        let plan = scatter_plan(&[4, 5, 6, 7], &[10, 20, 30], |_| 0);
        let channels: Vec<_> = plan.iter().map(|&(_, c)| c).collect();
        assert_eq!(plan.iter().map(|&(s, _)| s).collect::<Vec<_>>(), [4, 5, 6, 7]);
        assert_eq!(channels[..3].iter().collect::<std::collections::HashSet<_>>().len(), 3);
        assert!(scatter_plan(&[1], &[], |_| 0).is_empty());
    }

    #[test]
    fn keys_round_trip() {
        for set in BallSet::ALL {
            assert_eq!(BallSet::from_key(set.key()), Some(set));
        }
        assert_eq!(AreaKind::from_key("category"), Some(AreaKind::Category));
    }
}
