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
