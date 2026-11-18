//! The shop's shelf, and what each player owns from it.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, Row, params};

use super::to_sql;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ItemRow {
    pub(crate) id: i64,
    pub(crate) emoji: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) price: i64,
    pub(crate) listed: bool,
}

impl ItemRow {
    /// "🫘 Senzu Bean"
    pub(crate) fn label(&self) -> String {
        format!("{} {}", self.emoji, self.name)
    }
}

/// An item as staff wrote it.
pub(crate) struct ItemFields<'a> {
    pub(crate) emoji: &'a str,
    pub(crate) name: &'a str,
    pub(crate) description: &'a str,
    pub(crate) price: i64,
    pub(crate) listed: bool,
}

const COLUMNS: &str = "id, emoji, name, description, price, listed";

fn item(row: &Row) -> rusqlite::Result<ItemRow> {
    Ok(ItemRow { id: row.get(0)?, emoji: row.get(1)?, name: row.get(2)?, description: row.get(3)?, price: row.get(4)?, listed: row.get::<_, i64>(5)? != 0 })
}

/// Cheapest first.
pub(crate) fn all(conn: &Connection) -> rusqlite::Result<Vec<ItemRow>> {
    let mut stmt = conn.prepare_cached(&format!("SELECT {COLUMNS} FROM shop_items ORDER BY price, name"))?;
    stmt.query_map([], item)?.collect()
}

pub(crate) fn get(conn: &Connection, id: i64) -> rusqlite::Result<Option<ItemRow>> {
    conn.query_row(&format!("SELECT {COLUMNS} FROM shop_items WHERE id = ?1"), [id], item).optional()
}

/// The item with this name, any case.
pub(crate) fn named(conn: &Connection, name: &str) -> rusqlite::Result<Option<i64>> {
    conn.query_row("SELECT id FROM shop_items WHERE name = ?1", [name], |row| row.get(0)).optional()
}

pub(crate) fn insert(conn: &Connection, f: &ItemFields) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO shop_items (emoji, name, description, price, listed) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![f.emoji, f.name, f.description, f.price, i64::from(f.listed)],
    )?;
    Ok(conn.last_insert_rowid())
}

pub(crate) fn update(conn: &Connection, id: i64, f: &ItemFields) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE shop_items SET emoji = ?2, name = ?3, description = ?4, price = ?5, listed = ?6 WHERE id = ?1",
        params![id, f.emoji, f.name, f.description, f.price, i64::from(f.listed)],
    )?;
    Ok(())
}

/// Gone from the shop and from every inventory.
pub(crate) fn delete(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM owned_items WHERE item_id = ?1", [id])?;
    conn.execute("DELETE FROM shop_items WHERE id = ?1", [id])?;
    Ok(())
}

/// How many players own each item.
pub(crate) fn owners(conn: &Connection) -> rusqlite::Result<HashMap<i64, usize>> {
    let mut stmt = conn.prepare_cached("SELECT item_id, COUNT(*) FROM owned_items GROUP BY item_id")?;
    stmt.query_map([], |row| Ok((row.get(0)?, usize::try_from(row.get::<_, i64>(1)?).unwrap_or_default())))?.collect()
}

/// A player's inventory, cheapest first.
pub(crate) fn owned(conn: &Connection, player_id: u64) -> rusqlite::Result<Vec<(ItemRow, i64)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT s.id, s.emoji, s.name, s.description, s.price, s.listed, i.count FROM owned_items i \
         JOIN shop_items s ON s.id = i.item_id WHERE i.player_id = ?1 ORDER BY s.price, s.name",
    )?;
    stmt.query_map([to_sql(player_id)], |row| Ok((item(row)?, row.get(6)?)))?.collect()
}

pub(crate) fn count(conn: &Connection, player_id: u64, item_id: i64) -> rusqlite::Result<i64> {
    let n = conn
        .query_row("SELECT count FROM owned_items WHERE player_id = ?1 AND item_id = ?2", params![to_sql(player_id), item_id], |row| row.get(0))
        .optional()?;
    Ok(n.unwrap_or(0))
}

/// Exactly `count` of it; 0 takes it out of the inventory.
pub(crate) fn set_count(conn: &Connection, player_id: u64, item_id: i64, count: i64) -> rusqlite::Result<()> {
    if count == 0 {
        conn.execute("DELETE FROM owned_items WHERE player_id = ?1 AND item_id = ?2", params![to_sql(player_id), item_id])?;
        return Ok(());
    }
    conn.execute(
        "INSERT INTO owned_items (player_id, item_id, count) VALUES (?1, ?2, ?3) ON CONFLICT (player_id, item_id) DO UPDATE SET count = excluded.count",
        params![to_sql(player_id), item_id, count],
    )?;
    Ok(())
}
