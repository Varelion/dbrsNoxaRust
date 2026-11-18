//! SQLite, and nothing above it touches SQL. One file, one connection, behind a mutex: the bot serves
//! one server, and every query takes microseconds, so a pool would only cost memory.
//!
//! ```text
//!   services ─► Store::read / Store::write ─► repos (characters, spirit, feats, settings) ─► rusqlite ─► noxadbrs.db
//! ```

pub(crate) mod balls;
pub(crate) mod belongings;
pub(crate) mod characters;
pub(crate) mod expressions;
pub(crate) mod feats;
pub(crate) mod mcguffins;
pub(crate) mod players;
pub(crate) mod proxied;
pub(crate) mod settings;
pub(crate) mod shop;
pub(crate) mod spirit;
pub(crate) mod tutorials;
pub(crate) mod worlds;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use anyhow::{Context, Result};
use rusqlite::{Connection, Transaction};
use tracing::info;

// Each entry is applied once, in order; PRAGMA user_version counts how many have run.
const MIGRATIONS: &[&str] = &[include_str!("../../migrations/0001_init.sql"), include_str!("../../migrations/0002_profiles.sql"),
    include_str!("../../migrations/0003_ball_areas.sql"),
    include_str!("../../migrations/0004_mcguffins_npcs.sql"),
    include_str!("../../migrations/0005_pictures.sql"),
    include_str!("../../migrations/0006_places.sql"),
    include_str!("../../migrations/0007_posting.sql"),
    include_str!("../../migrations/0008_shop.sql"),
];

// Waits this long for a lock held by a backup or another reader instead of failing at once.
const BUSY_TIMEOUT_MS: u32 = 5_000;

pub(crate) struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "busy_timeout", BUSY_TIMEOUT_MS)?;
        Self::ready(conn)
    }

    /// A throwaway database, for tests.
    #[cfg(test)]
    pub(crate) fn memory() -> Result<Self> {
        Self::ready(Connection::open_in_memory()?)
    }

    fn ready(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate(&mut conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub(crate) fn read<T>(&self, query: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T> {
        Ok(query(&self.lock())?)
    }

    /// All or nothing: the closure's writes commit together, or none do.
    pub(crate) fn write<T>(&self, change: impl FnOnce(&Transaction) -> rusqlite::Result<T>) -> Result<T> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let value = change(&tx)?;
        tx.commit()?;
        Ok(value)
    }

    // A panic mid-query leaves the connection usable: SQLite rolled the transaction back.
    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let done: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if usize::try_from(done)? < MIGRATIONS.len() {
        info!(from = done, to = MIGRATIONS.len(), "migrating the database");
    }
    for (number, sql) in (1_i64..).zip(MIGRATIONS).skip(usize::try_from(done)?) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql).with_context(|| format!("migration {number}"))?;
        tx.pragma_update(None, "user_version", number)?;
        tx.commit()?;
    }
    Ok(())
}

// Snowflakes are below 2^63, so they convert to SQLite's INTEGER and back without loss.
pub(crate) const fn to_sql(id: u64) -> i64 {
    id as i64
}

pub(crate) const fn from_sql(id: i64) -> u64 {
    id as u64
}

/// A row limit as SQLite binds it.
pub(crate) fn limit_sql(limit: usize) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_run_once() {
        let store = Store::memory().unwrap();
        let version: i64 = store.read(|c| c.pragma_query_value(None, "user_version", |r| r.get(0))).unwrap();
        assert_eq!(version, limit_sql(MIGRATIONS.len()));

        let mut conn = store.lock();
        migrate(&mut conn).unwrap();
    }
}
