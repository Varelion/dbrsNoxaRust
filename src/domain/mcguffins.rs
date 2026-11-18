//! McGuffins: plot items staff hand out by hand, shown on the holder's card.

pub(crate) const NAME_MAX: usize = 60;
pub(crate) const DESCRIPTION_MAX: usize = 300;
pub(crate) const EMOJI_MAX: usize = 8;
pub(crate) const DEFAULT_EMOJI: &str = "🔮";

/// One holder at a time (giving it moves it), or any number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Holding {
    One,
    Many,
}

impl Holding {
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::One => "one",
            Self::Many => "many",
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::One => "One holder at a time",
            Self::Many => "Any number of holders",
        }
    }

    pub(crate) fn from_key(key: &str) -> Option<Self> {
        [Self::One, Self::Many].into_iter().find(|holding| holding.key() == key)
    }
}

/// Where a McGuffin lies while nobody carries it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    /// Somewhere in a world: a category.
    World(u64),
    /// One channel, picked by staff.
    Channel(u64),
    /// A channel rolled from every roleplay channel.
    Random(u64),
}

const WORLD: &str = "world";
const CHANNEL: &str = "channel";
const RANDOM: &str = "random";

impl Place {
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::World(_) => WORLD,
            Self::Channel(_) => CHANNEL,
            Self::Random(_) => RANDOM,
        }
    }

    /// The category or channel it names.
    pub(crate) const fn id(self) -> u64 {
        match self {
            Self::World(id) | Self::Channel(id) | Self::Random(id) => id,
        }
    }

    /// As stored: (kind, id). An unknown kind reads as nowhere rather than failing.
    pub(crate) fn from_parts(kind: &str, id: u64) -> Option<Self> {
        match kind {
            WORLD => Some(Self::World(id)),
            CHANNEL => Some(Self::Channel(id)),
            RANDOM => Some(Self::Random(id)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_reads_back_as_stored() {
        for place in [Place::World(1), Place::Channel(2), Place::Random(3)] {
            assert_eq!(Place::from_parts(place.key(), place.id()), Some(place));
        }
        assert_eq!(Place::from_parts("moon", 4), None);
    }
}
