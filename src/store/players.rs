//! Players: everyone with a wallet or a character. The cast (NPCs) is not a player.

use rusqlite::Connection;

use super::{from_sql, to_sql};
use crate::domain::npcs::CAST;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlayerRow {
    pub(crate) player_id: u64,
    pub(crate) sp: i64,
    pub(crate) characters: i64,
}

pub(crate) fn all(conn: &Connection) -> rusqlite::Result<Vec<PlayerRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT p.player_id, ifnull(w.sp, 0), (SELECT count(*) FROM characters c WHERE c.player_id = p.player_id)
         FROM (SELECT player_id FROM wallets UNION SELECT player_id FROM characters) p
         LEFT JOIN wallets w ON w.player_id = p.player_id
         WHERE p.player_id != ?1 ORDER BY p.player_id",
    )?;
    stmt.query_map([to_sql(CAST)], |row| Ok(PlayerRow { player_id: from_sql(row.get(0)?), sp: row.get(1)?, characters: row.get(2)? }))?.collect()
}
