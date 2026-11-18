//! What a character carries and which Dragon Balls they hold.

use rusqlite::Connection;

/// (item key, quantity), item order.
pub(crate) fn items(conn: &Connection, character_id: i64) -> rusqlite::Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare_cached("SELECT item, quantity FROM inventory WHERE character_id = ?1 ORDER BY item")?;
    stmt.query_map([character_id], |row| Ok((row.get(0)?, row.get(1)?)))?.collect()
}

/// (set key, star) held by the character, set then star order.
pub(crate) fn balls(conn: &Connection, character_id: i64) -> rusqlite::Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare_cached("SELECT set_key, star FROM dragon_balls WHERE holder_id = ?1 ORDER BY set_key, star")?;
    stmt.query_map([character_id], |row| Ok((row.get(0)?, row.get(1)?)))?.collect()
}
