//! Dragon Ball sets: their balls, their search areas and the roll they need.

use rusqlite::{Connection, OptionalExtension, params};

use super::{from_sql, to_sql};
use crate::domain::dragon_balls::STARS;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BallRow {
    pub(crate) star: i64,
    pub(crate) holder: Option<(i64, String, u64)>,
    pub(crate) channel_id: Option<u64>,
}

/// Every set has all seven balls, unplaced until staff hide them.
pub(crate) fn ensure_set(conn: &Connection, set: &str) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare_cached("INSERT INTO dragon_balls (set_key, star) VALUES (?1, ?2) ON CONFLICT DO NOTHING")?;
    for star in STARS {
        stmt.execute(params![set, star])?;
    }
    Ok(())
}

pub(crate) fn balls(conn: &Connection, set: &str) -> rusqlite::Result<Vec<BallRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT b.star, c.id, c.name, c.player_id, b.channel_id FROM dragon_balls b
         LEFT JOIN characters c ON c.id = b.holder_id WHERE b.set_key = ?1 ORDER BY b.star",
    )?;
    stmt.query_map([set], |row| {
        let holder = match row.get::<_, Option<i64>>(1)? {
            Some(id) => Some((id, row.get(2)?, from_sql(row.get(3)?))),
            None => None,
        };
        Ok(BallRow { star: row.get(0)?, holder, channel_id: row.get::<_, Option<i64>>(4)?.map(from_sql) })
    })?
    .collect()
}

/// Hides a ball in a channel; false when someone holds it.
pub(crate) fn place(conn: &Connection, set: &str, star: i64, channel_id: u64) -> rusqlite::Result<bool> {
    let moved = conn.execute(
        "UPDATE dragon_balls SET channel_id = ?3 WHERE set_key = ?1 AND star = ?2 AND holder_id IS NULL",
        params![set, star, to_sql(channel_id)],
    )?;
    Ok(moved > 0)
}

/// (channel or category id, kind key).
pub(crate) fn areas(conn: &Connection, set: &str) -> rusqlite::Result<Vec<(u64, String)>> {
    let mut stmt = conn.prepare_cached("SELECT channel_id, kind FROM ball_areas WHERE set_key = ?1 ORDER BY kind, channel_id")?;
    stmt.query_map([set], |row| Ok((from_sql(row.get(0)?), row.get(1)?)))?.collect()
}

pub(crate) fn replace_areas(conn: &Connection, set: &str, areas: &[(u64, &str)]) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM ball_areas WHERE set_key = ?1", [set])?;
    let mut stmt = conn.prepare_cached("INSERT INTO ball_areas (set_key, channel_id, kind) VALUES (?1, ?2, ?3)")?;
    for (channel_id, kind) in areas {
        stmt.execute(params![set, to_sql(*channel_id), kind])?;
    }
    Ok(())
}

pub(crate) fn need(conn: &Connection, set: &str) -> rusqlite::Result<Option<i64>> {
    conn.query_row("SELECT need FROM ball_rules WHERE set_key = ?1", [set], |row| row.get(0)).optional()
}

pub(crate) fn set_need(conn: &Connection, set: &str, need: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO ball_rules (set_key, need) VALUES (?1, ?2) ON CONFLICT (set_key) DO UPDATE SET need = excluded.need",
        params![set, need],
    )?;
    Ok(())
}
