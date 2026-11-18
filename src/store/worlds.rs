//! Worlds: the categories staff mark as worlds.

use rusqlite::Connection;

use super::{from_sql, to_sql};

pub(crate) fn all(conn: &Connection) -> rusqlite::Result<Vec<u64>> {
    let mut stmt = conn.prepare_cached("SELECT category_id FROM worlds ORDER BY category_id")?;
    stmt.query_map([], |row| Ok(from_sql(row.get(0)?)))?.collect()
}

/// The worlds become exactly these.
pub(crate) fn replace(conn: &Connection, categories: &[u64]) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM worlds", [])?;
    let mut insert = conn.prepare_cached("INSERT INTO worlds (category_id) VALUES (?1) ON CONFLICT DO NOTHING")?;
    for category in categories {
        insert.execute([to_sql(*category)])?;
    }
    Ok(())
}
