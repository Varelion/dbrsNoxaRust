//! McGuffins and who holds them. Retiring sets retired_*; nothing deletes one.

use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{from_sql, to_sql};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct McguffinRow {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) emoji: String,
    pub(crate) description: String,
    pub(crate) holding: String,
    pub(crate) retired: bool,
}

/// (character id, character name, player id).
pub(crate) type HolderRow = (i64, String, u64);

const COLUMNS: &str = "id, name, emoji, description, holding, retired_at IS NOT NULL";

fn row_of(row: &Row) -> rusqlite::Result<McguffinRow> {
    Ok(McguffinRow { id: row.get(0)?, name: row.get(1)?, emoji: row.get(2)?, description: row.get(3)?, holding: row.get(4)?, retired: row.get(5)? })
}

pub(crate) struct McguffinFields<'a> {
    pub(crate) name: &'a str,
    pub(crate) emoji: &'a str,
    pub(crate) description: &'a str,
    pub(crate) holding: &'a str,
}

/// None when the name is taken (any case).
pub(crate) fn insert(conn: &Connection, f: &McguffinFields, staff_id: u64, at: i64) -> rusqlite::Result<Option<i64>> {
    let added = conn.execute(
        "INSERT INTO mcguffins (name, emoji, description, holding, created_by, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT DO NOTHING",
        params![f.name, f.emoji, f.description, f.holding, to_sql(staff_id), at],
    )?;
    Ok((added > 0).then(|| conn.last_insert_rowid()))
}

/// False when another McGuffin has the name.
pub(crate) fn update(conn: &Connection, id: i64, f: &McguffinFields) -> rusqlite::Result<bool> {
    let clash: Option<i64> =
        conn.query_row("SELECT id FROM mcguffins WHERE lower(name) = lower(?1) AND id != ?2", params![f.name, id], |r| r.get(0)).optional()?;
    if clash.is_some() {
        return Ok(false);
    }
    conn.execute(
        "UPDATE mcguffins SET name = ?2, emoji = ?3, description = ?4, holding = ?5 WHERE id = ?1",
        params![id, f.name, f.emoji, f.description, f.holding],
    )?;
    Ok(true)
}

pub(crate) fn get(conn: &Connection, id: i64) -> rusqlite::Result<Option<McguffinRow>> {
    conn.query_row(&format!("SELECT {COLUMNS} FROM mcguffins WHERE id = ?1"), [id], row_of).optional()
}

/// In play first, then retired; by name.
pub(crate) fn all(conn: &Connection) -> rusqlite::Result<Vec<McguffinRow>> {
    let sql = format!("SELECT {COLUMNS} FROM mcguffins ORDER BY retired_at IS NOT NULL, lower(name)");
    conn.prepare_cached(&sql)?.query_map([], row_of)?.collect()
}

pub(crate) fn set_retired(conn: &Connection, id: i64, staff_id: Option<u64>, at: Option<i64>) -> rusqlite::Result<()> {
    conn.execute("UPDATE mcguffins SET retired_by = ?2, retired_at = ?3 WHERE id = ?1", params![id, staff_id.map(to_sql), at])?;
    Ok(())
}

pub(crate) fn holders(conn: &Connection, id: i64) -> rusqlite::Result<Vec<HolderRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT c.id, c.name, c.player_id FROM mcguffin_holders h JOIN characters c ON c.id = h.character_id WHERE h.mcguffin_id = ?1 ORDER BY h.given_at",
    )?;
    stmt.query_map([id], |row| Ok((row.get(0)?, row.get(1)?, from_sql(row.get(2)?))))?.collect()
}

/// False when the character already holds it.
pub(crate) fn give(conn: &Connection, id: i64, character_id: i64, staff_id: u64, at: i64) -> rusqlite::Result<bool> {
    let added = conn.execute(
        "INSERT INTO mcguffin_holders (mcguffin_id, character_id, given_by, given_at) VALUES (?1, ?2, ?3, ?4) ON CONFLICT DO NOTHING",
        params![id, character_id, to_sql(staff_id), at],
    )?;
    Ok(added > 0)
}

pub(crate) fn take(conn: &Connection, id: i64, character_id: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute("DELETE FROM mcguffin_holders WHERE mcguffin_id = ?1 AND character_id = ?2", params![id, character_id])? > 0)
}

/// (emoji, name) of what a character holds, in play only.
pub(crate) fn held_by(conn: &Connection, character_id: i64) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT m.emoji, m.name FROM mcguffin_holders h JOIN mcguffins m ON m.id = h.mcguffin_id
         WHERE h.character_id = ?1 AND m.retired_at IS NULL ORDER BY lower(m.name)",
    )?;
    stmt.query_map([character_id], |row| Ok((row.get(0)?, row.get(1)?)))?.collect()
}
