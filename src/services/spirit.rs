//! Spirit Points, for staff to give, take, set or grant, and for a player to read. Every change leaves a
//! ledger line with its memo; the Discord layer posts it to the mod log as well.
//!
//! ```text
//!   change(give 5 to ann, "Halloween event") ─► Changed { before: 31, after: 36 }
//!   grant(5 to [ann, bob], "Stimulus check")  ─► Granted { count: 2 }
//! ```

use anyhow::Result;

use crate::domain::spirit::{MEMO_MAX, SpAction, balance_after, valid_amount};
use crate::store::spirit::{self as repo, LedgerEntry, LedgerRow};
use crate::store::Store;

use super::{Clock, clean_line};

/// One player's change, as staff typed it.
pub(crate) struct Change<'a> {
    pub(crate) action: SpAction,
    pub(crate) player_id: u64,
    pub(crate) amount: i64,
    pub(crate) memo: &'a str,
    pub(crate) staff_id: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Changed {
    Done { before: i64, after: i64 },
    NoMemo,
    BadAmount,
    TooLow { before: i64 },
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Granted {
    Done { count: usize },
    NoMemo,
    BadAmount,
    NoPlayers,
}

pub(crate) struct Spirit<'a> {
    store: &'a Store,
    clock: &'a Clock,
}

impl<'a> Spirit<'a> {
    pub(crate) const fn new(store: &'a Store, clock: &'a Clock) -> Self {
        Self { store, clock }
    }

    pub(crate) fn balance(&self, player_id: u64) -> Result<i64> {
        self.store.read(|c| repo::balance(c, player_id))
    }

    /// Newest first.
    pub(crate) fn history(&self, player_id: u64, limit: usize) -> Result<Vec<LedgerRow>> {
        self.store.read(|c| repo::history(c, player_id, limit))
    }

    pub(crate) fn change(&self, change: &Change) -> Result<Changed> {
        let memo = clean_line(change.memo, MEMO_MAX);
        if memo.is_empty() {
            return Ok(Changed::NoMemo);
        }
        if !valid_amount(change.action, change.amount) {
            return Ok(Changed::BadAmount);
        }

        let at = self.clock.now();
        // Read and write in one transaction, so two staff changing one wallet at once cannot lose a change.
        self.store.write(|tx| {
            let before = repo::balance(tx, change.player_id)?;
            let Some(after) = balance_after(change.action, before, change.amount) else {
                return Ok(Changed::TooLow { before });
            };
            let entry = LedgerEntry {
                player_id: change.player_id,
                delta: after - before,
                balance: after,
                kind: change.action.key(),
                memo: &memo,
                staff_id: change.staff_id,
                at,
            };
            repo::record(tx, &entry)?;
            Ok(Changed::Done { before, after })
        })
    }

    /// Every holder of a role gets the same amount, each with a ledger line of their own; all or none.
    pub(crate) fn grant(&self, player_ids: &[u64], amount: i64, memo: &str, staff_id: u64) -> Result<Granted> {
        let memo = clean_line(memo, MEMO_MAX);
        if memo.is_empty() {
            return Ok(Granted::NoMemo);
        }
        if !valid_amount(SpAction::Grant, amount) {
            return Ok(Granted::BadAmount);
        }
        let mut unique = player_ids.to_vec();
        unique.sort_unstable();
        unique.dedup();
        if unique.is_empty() {
            return Ok(Granted::NoPlayers);
        }

        let at = self.clock.now();
        self.store.write(|tx| {
            for &player_id in &unique {
                let before = repo::balance(tx, player_id)?;
                let after = before + amount;
                let entry =
                    LedgerEntry { player_id, delta: amount, balance: after, kind: SpAction::Grant.key(), memo: &memo, staff_id, at };
                repo::record(tx, &entry)?;
            }
            Ok(Granted::Done { count: unique.len() })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::spirit::SP_MAX;
    use crate::services::Services;

    const NOW: i64 = 1_795_000_000_000;
    const ANN: u64 = 42;
    const BOB: u64 = 43;
    const STAFF: u64 = 9;

    fn change<'a>(action: SpAction, amount: i64, memo: &'a str) -> Change<'a> {
        Change { action, player_id: ANN, amount, memo, staff_id: STAFF }
    }

    #[test]
    fn give_take_and_set_leave_ledger_lines_newest_first() {
        let services = Services::memory(NOW);
        let sp = services.spirit();

        assert_eq!(sp.change(&change(SpAction::Give, 31, "Shop launch")).unwrap(), Changed::Done { before: 0, after: 31 });
        assert_eq!(sp.change(&change(SpAction::Take, 5, "Refund reversed")).unwrap(), Changed::Done { before: 31, after: 26 });
        assert_eq!(sp.change(&change(SpAction::Set, 10, "Audit  fix")).unwrap(), Changed::Done { before: 26, after: 10 });
        assert_eq!(sp.balance(ANN).unwrap(), 10);
        assert_eq!(sp.balance(BOB).unwrap(), 0);

        let lines: Vec<_> = sp.history(ANN, 10).unwrap().into_iter().map(|l| (l.kind, l.delta, l.balance, l.memo)).collect();
        assert_eq!(
            lines,
            [
                ("set".to_owned(), -16, 10, "Audit fix".to_owned()),
                ("take".to_owned(), -5, 26, "Refund reversed".to_owned()),
                ("give".to_owned(), 31, 31, "Shop launch".to_owned()),
            ]
        );
    }

    #[test]
    fn refused_changes_write_nothing() {
        let services = Services::memory(NOW);
        let sp = services.spirit();
        sp.change(&change(SpAction::Give, 3, "Event")).unwrap();

        assert_eq!(sp.change(&change(SpAction::Give, 5, "   ")).unwrap(), Changed::NoMemo);
        assert_eq!(sp.change(&change(SpAction::Give, 0, "Event")).unwrap(), Changed::BadAmount);
        assert_eq!(sp.change(&change(SpAction::Give, SP_MAX + 1, "Typo")).unwrap(), Changed::BadAmount);
        assert_eq!(sp.change(&change(SpAction::Take, 5, "Oops")).unwrap(), Changed::TooLow { before: 3 });

        assert_eq!(sp.balance(ANN).unwrap(), 3);
        assert_eq!(sp.history(ANN, 10).unwrap().len(), 1);
    }

    #[test]
    fn a_grant_gives_each_holder_once() {
        let services = Services::memory(NOW);
        let sp = services.spirit();
        sp.change(&change(SpAction::Give, 2, "Event")).unwrap();

        assert_eq!(sp.grant(&[ANN, BOB, ANN], 5, "Stimulus check", STAFF).unwrap(), Granted::Done { count: 2 });
        assert_eq!(sp.balance(ANN).unwrap(), 7);
        assert_eq!(sp.balance(BOB).unwrap(), 5);
        assert_eq!(sp.history(BOB, 1).unwrap()[0].kind, "grant");

        assert_eq!(sp.grant(&[], 5, "Nobody", STAFF).unwrap(), Granted::NoPlayers);
        assert_eq!(sp.grant(&[ANN], 5, "", STAFF).unwrap(), Granted::NoMemo);
    }
}
