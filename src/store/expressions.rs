//! Expressions: named pictures a character posts with. Names are stored lowercase.

use rusqlite::{Connection, OptionalExtension, Row, params};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExpressionRow {
    pub(crate) name: String,
    pub(crate) url: String,
    pub(crate) color: Option<u32>,
}

fn row_of(row: &Row) -> rusqlite::Result<ExpressionRow> {
    Ok(ExpressionRow { name: row.get(0)?, url: row.get(1)?, color: row.get(2)? })
}

pub(crate) fn of_character(conn: &Connection, character_id: i64) -> rusqlite::Result<Vec<ExpressionRow>> {
    let mut stmt = conn.prepare_cached("SELECT name, url, color FROM expressions WHERE character_id = ?1 ORDER BY name")?;
    stmt.query_map([character_id], row_of)?.collect()
}

pub(crate) fn get(conn: &Connection, character_id: i64, name: &str) -> rusqlite::Result<Option<ExpressionRow>> {
    conn.query_row("SELECT name, url, color FROM expressions WHERE character_id = ?1 AND name = ?2", params![character_id, name], row_of).optional()
}

pub(crate) fn save(conn: &Connection, character_id: i64, e: &ExpressionRow) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO expressions (character_id, name, url, color) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (character_id, name) DO UPDATE SET url = excluded.url, color = excluded.color",
        params![character_id, e.name, e.url, e.color],
    )?;
    Ok(())
}

/// False when there was none.
pub(crate) fn remove(conn: &Connection, character_id: i64, name: &str) -> rusqlite::Result<bool> {
    Ok(conn.execute("DELETE FROM expressions WHERE character_id = ?1 AND name = ?2", params![character_id, name])? > 0)
}
