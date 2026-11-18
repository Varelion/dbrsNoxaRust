//! A feat: what a character did, as staff saw it, with the posts that show it. Silent: players never
//! see one. Staff read them beside a tier-up request, to check the claims.
//!
//! ```text
//!   add ─► active ─┬─► edit · link · unlink ─┐
//!                  └─► archive ─► archived ──┴─► restore ─► active      nothing deletes a feat
//! ```

pub(crate) const NOTE_MAX: usize = 500;
pub(crate) const REASON_MAX: usize = 200;
pub(crate) const LINKS_MAX: usize = 10;

// Staff call a feat by its code: 12 ─► "F-0012".
const CODE_PREFIX: &str = "F-";
const CODE_WIDTH: usize = 4;

/// Defines an enum whose variants each have a stored key and a label staff read.
macro_rules! keyed {
    ($name:ident { $($variant:ident => $key:literal, $label:literal;)+ }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(crate) enum $name {
            $($variant,)+
        }

        impl $name {
            pub(crate) const ALL: &'static [Self] = &[$(Self::$variant,)+];

            pub(crate) const fn key(self) -> &'static str {
                match self {
                    $(Self::$variant => $key,)+
                }
            }

            pub(crate) const fn label(self) -> &'static str {
                match self {
                    $(Self::$variant => $label,)+
                }
            }

            pub(crate) fn from_key(key: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|value| value.key() == key)
            }
        }
    };
}

keyed!(FeatKind {
    Victory => "victory", "⚔️ Victory";
    Defeat => "defeat", "🩸 Defeat";
    Development => "development", "🌱 Development";
    Trigger => "trigger", "⚡ Trigger";
    Injury => "injury", "🩹 Injury";
    Death => "death", "💀 Death";
    Plot => "plot", "📜 Plot";
    Note => "note", "📝 Note";
});

// What the feat stands for in a tier-up: a fight, development, support, or nothing.
keyed!(FeatCounts {
    Fight => "fight", "Fight";
    Development => "development", "Development";
    Support => "support", "Support";
    Nothing => "none", "—";
});

/// Active feats show in lists and reviews; archived ones wait to be restored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FeatState {
    Active,
    Archived,
}

impl FeatState {
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
        }
    }

    pub(crate) fn from_key(key: &str) -> Option<Self> {
        [Self::Active, Self::Archived].into_iter().find(|state| state.key() == key)
    }
}

pub(crate) fn feat_code(id: i64) -> String {
    format!("{CODE_PREFIX}{id:0CODE_WIDTH$}")
}

/// "F-0012", "f12" or "12" ─► 12; `None` for anything else.
pub(crate) fn feat_id_of(text: &str) -> Option<i64> {
    let lower = text.trim().to_ascii_lowercase();
    let digits = lower.strip_prefix("f-").or_else(|| lower.strip_prefix('f')).unwrap_or(&lower);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        assert_eq!(feat_code(12), "F-0012");
        assert_eq!(feat_code(12345), "F-12345");
        assert_eq!(feat_id_of("F-0012"), Some(12));
        assert_eq!(feat_id_of("f12"), Some(12));
        assert_eq!(feat_id_of(" 12 "), Some(12));
        assert_eq!(feat_id_of("Itami"), None);
        assert_eq!(feat_id_of("F-"), None);
    }

    #[test]
    fn kinds_and_counts_round_trip() {
        for kind in FeatKind::ALL {
            assert_eq!(FeatKind::from_key(kind.key()), Some(*kind));
        }
        for counts in FeatCounts::ALL {
            assert_eq!(FeatCounts::from_key(counts.key()), Some(*counts));
        }
        assert_eq!(FeatKind::from_key("brawl"), None);
        assert_eq!(FeatState::from_key("archived"), Some(FeatState::Archived));
    }
}
