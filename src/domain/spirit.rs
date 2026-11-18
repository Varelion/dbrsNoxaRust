//! Spirit Points: a player's currency. Posting earns none; only staff change a balance.
//!
//! ```text
//!   give 5   31 ─► 36
//!   take 5   31 ─► 26     take 40 from 31 is refused: no balance goes below 0
//!   set 10   31 ─► 10
//!   grant 5  every holder of a role, as give
//! ```

/// The most one command may move: a typo of extra zeros is refused, not minted.
pub(crate) const SP_MAX: i64 = 1000;
pub(crate) const MEMO_MAX: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SpAction {
    Give,
    Take,
    Set,
    Grant,
}

impl SpAction {
    pub(crate) const ALL: [Self; 4] = [Self::Give, Self::Take, Self::Set, Self::Grant];

    /// The name stored in the ledger and typed as a subcommand.
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Give => "give",
            Self::Take => "take",
            Self::Set => "set",
            Self::Grant => "grant",
        }
    }

    pub(crate) fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.key() == key)
    }

    // Set may empty a wallet; every other action moves at least 1.
    const fn least(self) -> i64 {
        match self {
            Self::Set => 0,
            _ => 1,
        }
    }
}

pub(crate) fn valid_amount(action: SpAction, amount: i64) -> bool {
    amount >= action.least() && amount <= SP_MAX
}

/// The balance after the action; `None` when it would fall below 0.
pub(crate) fn balance_after(action: SpAction, balance: i64, amount: i64) -> Option<i64> {
    match action {
        SpAction::Set => Some(amount),
        SpAction::Take => {
            let left = balance - amount;
            if left < 0 {
                return None;
            }
            Some(left)
        }
        SpAction::Give | SpAction::Grant => Some(balance + amount),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn give_adds_take_refuses_below_zero_set_replaces() {
        assert_eq!(balance_after(SpAction::Give, 31, 5), Some(36));
        assert_eq!(balance_after(SpAction::Take, 31, 5), Some(26));
        assert_eq!(balance_after(SpAction::Take, 31, 31), Some(0));
        assert_eq!(balance_after(SpAction::Take, 31, 40), None);
        assert_eq!(balance_after(SpAction::Set, 31, 10), Some(10));
        assert_eq!(balance_after(SpAction::Grant, 0, 5), Some(5));
    }

    #[test]
    fn amounts_are_one_to_max_and_set_may_be_zero() {
        assert!(valid_amount(SpAction::Give, 1));
        assert!(!valid_amount(SpAction::Give, 0));
        assert!(valid_amount(SpAction::Set, 0));
        assert!(!valid_amount(SpAction::Give, -3));
        assert!(valid_amount(SpAction::Give, SP_MAX));
        assert!(!valid_amount(SpAction::Give, SP_MAX + 1));
    }

    #[test]
    fn keys_round_trip() {
        for action in SpAction::ALL {
            assert_eq!(SpAction::from_key(action.key()), Some(action));
        }
        assert_eq!(SpAction::from_key("steal"), None);
    }
}
