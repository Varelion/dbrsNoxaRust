//! The parts of posts made as characters: who wrote them, as whom, and through which webhook.

use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{from_sql, to_sql};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProxiedRow {
    pub(crate) message_id: u64,
    pub(crate) channel_id: u64,
    pub(crate) hook_channel: u64,
    pub(crate) thread_id: Option<u64>,
    pub(crate) webhook_id: u64,
    pub(crate) author_id: u64,
    pub(crate) character_id: i64,
    pub(crate) group_id: u64,
    pub(crate) part: i64,
    pub(crate) at: i64,
}

const COLUMNS: &str = "message_id, channel_id, hook_channel, thread_id, webhook_id, author_id, character_id, group_id, part, at";

fn row_of(row: &Row) -> rusqlite::Result<ProxiedRow> {
    Ok(ProxiedRow {
        message_id: from_sql(row.get(0)?),
        channel_id: from_sql(row.get(1)?),
        hook_channel: from_sql(row.get(2)?),
        thread_id: row.get::<_, Option<i64>>(3)?.map(from_sql),
        webhook_id: from_sql(row.get(4)?),
        author_id: from_sql(row.get(5)?),
        character_id: row.get(6)?,
        group_id: from_sql(row.get(7)?),
        part: row.get(8)?,
        at: row.get(9)?,
    })
}

pub(crate) fn record(conn: &Connection, r: &ProxiedRow) -> rusqlite::Result<()> {
    conn.execute(
        &format!("INSERT INTO proxied ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) ON CONFLICT DO NOTHING"),
        params![
            to_sql(r.message_id),
            to_sql(r.channel_id),
            to_sql(r.hook_channel),
            r.thread_id.map(to_sql),
            to_sql(r.webhook_id),
            to_sql(r.author_id),
            r.character_id,
            to_sql(r.group_id),
            r.part,
            r.at
        ],
    )?;
    Ok(())
}

pub(crate) fn get(conn: &Connection, message_id: u64) -> rusqlite::Result<Option<ProxiedRow>> {
    conn.query_row(&format!("SELECT {COLUMNS} FROM proxied WHERE message_id = ?1"), [to_sql(message_id)], row_of).optional()
}

/// Every part of a post, first to last.
pub(crate) fn group(conn: &Connection, group_id: u64) -> rusqlite::Result<Vec<ProxiedRow>> {
    let mut stmt = conn.prepare_cached(&format!("SELECT {COLUMNS} FROM proxied WHERE group_id = ?1 ORDER BY part"))?;
    stmt.query_map([to_sql(group_id)], row_of)?.collect()
}

pub(crate) fn forget(conn: &Connection, message_id: u64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM proxied WHERE message_id = ?1", [to_sql(message_id)])?;
    Ok(())
}
