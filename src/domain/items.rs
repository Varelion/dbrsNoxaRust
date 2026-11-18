//! The item catalog: what a character can carry. Stored by key, shown by label.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Item {
    /// Points the general way to the nearest Dragon Ball. Reusable.
    DragonRadar,
}

impl Item {
    const ALL: [Self; 1] = [Self::DragonRadar];

    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::DragonRadar => "dragon_radar",
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::DragonRadar => "📡 Dragon Radar",
        }
    }

    pub(crate) fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|item| item.key() == key)
    }
}
