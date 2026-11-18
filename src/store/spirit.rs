//! Wallets and their ledger. A balance and its ledger line are written in one transaction (Store::write).

use rusqlite::{Connection, OptionalExtension, params};

use super::{from_sql, limit_sql, to_sql};

pub(crate) struct LedgerEntry<'a> {
    pub(crate) player_id: u64,
    pub(crate) delta: i64,
    pub(crate) balance: i64,
    pub(crate) kind: &'a str,
    pub(crate) memo: &'a str,
    pub(crate) staff_id: u64,
    pub(crate) at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LedgerRow {
    pub(crate) delta: i64,
    pub(crate) balance: i64,
    pub(crate) kind: String,
    pub(crate) memo: String,
    pub(crate) staff_id: u64,
    pub(crate) at: i64,
}

pub(crate) fn balance(conn: &Connection, player_id: u64) -> rusqlite::Result<i64> {
    let sp = conn
        .query_row("SELECT sp FROM wallets WHERE player_id = ?1", [to_sql(player_id)], |row| row.get(0))
        .optional()?;
    Ok(sp.unwrap_or(0))
}

pub(crate) fn record(conn: &Connection, e: &LedgerEntry) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO wallets (player_id, sp) VALUES (?1, ?2) ON CONFLICT (player_id) DO UPDATE SET sp = excluded.sp",
        params![to_sql(e.player_id), e.balance],
    )?;
    conn.execute(
        "INSERT INTO ledger (player_id, delta, balance, kind, memo, staff_id, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![to_sql(e.player_id), e.delta, e.balance, e.kind, e.memo, to_sql(e.staff_id), e.at],
    )?;
    Ok(())
}

/// Newest first.
pub(crate) fn history(conn: &Connection, player_id: u64, limit: usize) -> rusqlite::Result<Vec<LedgerRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT delta, balance, kind, memo, staff_id, at FROM ledger WHERE player_id = ?1 ORDER BY id DESC LIMIT ?2",
    )?;
    stmt.query_map(params![to_sql(player_id), limit_sql(limit)], |row| {
        Ok(LedgerRow {
            delta: row.get(0)?,
            balance: row.get(1)?,
            kind: row.get(2)?,
            memo: row.get(3)?,
            staff_id: from_sql(row.get(4)?),
            at: row.get(5)?,
        })
    })?
    .collect()
}
