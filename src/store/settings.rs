//! Key/value settings: the mod log channel, and whatever staff configure next.

use rusqlite::{Connection, OptionalExtension, params};

pub(crate) fn get(conn: &Connection, name: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM settings WHERE name = ?1", [name], |row| row.get(0)).optional()
}

pub(crate) fn set(conn: &Connection, name: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (name, value) VALUES (?1, ?2) ON CONFLICT (name) DO UPDATE SET value = excluded.value",
        params![name, value],
    )?;
    Ok(())
}
