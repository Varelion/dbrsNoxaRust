//! Feats and their links. Nothing here deletes a feat: archiving sets archived_*, restoring clears it.

use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{from_sql, limit_sql, to_sql};
use crate::domain::feats::FeatState;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FeatRow {
    pub(crate) id: i64,
    pub(crate) character_id: i64,
    pub(crate) character_name: String,
    pub(crate) player_id: u64,
    pub(crate) kind: String,
    pub(crate) counts: String,
    pub(crate) note: String,
    pub(crate) added_by: u64,
    pub(crate) added_at: i64,
    pub(crate) edited_by: Option<u64>,
    pub(crate) edited_at: Option<i64>,
    pub(crate) archived_by: Option<u64>,
    pub(crate) archived_at: Option<i64>,
    pub(crate) archive_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LinkRow {
    pub(crate) id: i64,
    pub(crate) channel_id: u64,
    pub(crate) message_id: u64,
}

// Every read joins the character, so a feat always shows its character's current name.
const SELECT: &str = "SELECT f.id, f.character_id, c.name, c.player_id, f.kind, f.counts, f.note, f.added_by, f.added_at,
        f.edited_by, f.edited_at, f.archived_by, f.archived_at, f.archive_reason
    FROM feats f JOIN characters c ON c.id = f.character_id";

fn row_of(row: &Row) -> rusqlite::Result<FeatRow> {
    Ok(FeatRow {
        id: row.get(0)?,
        character_id: row.get(1)?,
        character_name: row.get(2)?,
        player_id: from_sql(row.get(3)?),
        kind: row.get(4)?,
        counts: row.get(5)?,
        note: row.get(6)?,
        added_by: from_sql(row.get(7)?),
        added_at: row.get(8)?,
        edited_by: row.get::<_, Option<i64>>(9)?.map(from_sql),
        edited_at: row.get(10)?,
        archived_by: row.get::<_, Option<i64>>(11)?.map(from_sql),
        archived_at: row.get(12)?,
        archive_reason: row.get(13)?,
    })
}

const fn state_clause(state: FeatState) -> &'static str {
    match state {
        FeatState::Active => "f.archived_at IS NULL",
        FeatState::Archived => "f.archived_at IS NOT NULL",
    }
}

pub(crate) struct NewFeat<'a> {
    pub(crate) character_id: i64,
    pub(crate) kind: &'a str,
    pub(crate) counts: &'a str,
    pub(crate) note: &'a str,
    pub(crate) staff_id: u64,
    pub(crate) at: i64,
}

pub(crate) fn insert(conn: &Connection, f: &NewFeat) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO feats (character_id, kind, counts, note, added_by, added_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![f.character_id, f.kind, f.counts, f.note, to_sql(f.staff_id), f.at],
    )?;
    Ok(conn.last_insert_rowid())
}

pub(crate) fn get(conn: &Connection, id: i64) -> rusqlite::Result<Option<FeatRow>> {
    conn.query_row(&format!("{SELECT} WHERE f.id = ?1"), [id], row_of).optional()
}

/// A character's feats, oldest first, as a review reads them.
pub(crate) fn of_character(conn: &Connection, character_id: i64, state: FeatState) -> rusqlite::Result<Vec<FeatRow>> {
    let sql = format!("{SELECT} WHERE f.character_id = ?1 AND {} ORDER BY f.id", state_clause(state));
    conn.prepare_cached(&sql)?.query_map([character_id], row_of)?.collect()
}

/// Every character's, newest first: what a staff menu offers.
pub(crate) fn latest(conn: &Connection, state: FeatState, limit: usize) -> rusqlite::Result<Vec<FeatRow>> {
    let sql = format!("{SELECT} WHERE {} ORDER BY f.id DESC LIMIT ?1", state_clause(state));
    conn.prepare_cached(&sql)?.query_map([limit_sql(limit)], row_of)?.collect()
}

pub(crate) struct FeatFields<'a> {
    pub(crate) kind: &'a str,
    pub(crate) counts: &'a str,
    pub(crate) note: &'a str,
    pub(crate) staff_id: u64,
    pub(crate) at: i64,
}

pub(crate) fn update(conn: &Connection, id: i64, f: &FeatFields) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE feats SET kind = ?2, counts = ?3, note = ?4, edited_by = ?5, edited_at = ?6 WHERE id = ?1",
        params![id, f.kind, f.counts, f.note, to_sql(f.staff_id), f.at],
    )?;
    Ok(())
}

pub(crate) fn archive(conn: &Connection, id: i64, staff_id: u64, at: i64, reason: Option<&str>) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE feats SET archived_by = ?2, archived_at = ?3, archive_reason = ?4 WHERE id = ?1",
        params![id, to_sql(staff_id), at, reason],
    )?;
    Ok(())
}

/// Back as it was; edited_* records who restored it.
pub(crate) fn restore(conn: &Connection, id: i64, staff_id: u64, at: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE feats SET archived_by = NULL, archived_at = NULL, archive_reason = NULL, edited_by = ?2, edited_at = ?3 WHERE id = ?1",
        params![id, to_sql(staff_id), at],
    )?;
    Ok(())
}

pub(crate) fn links(conn: &Connection, feat_id: i64) -> rusqlite::Result<Vec<LinkRow>> {
    let mut stmt = conn.prepare_cached("SELECT id, channel_id, message_id FROM feat_links WHERE feat_id = ?1 ORDER BY id")?;
    stmt.query_map([feat_id], |row| {
        Ok(LinkRow { id: row.get(0)?, channel_id: from_sql(row.get(1)?), message_id: from_sql(row.get(2)?) })
    })?
    .collect()
}

pub(crate) struct NewLink {
    pub(crate) feat_id: i64,
    pub(crate) channel_id: u64,
    pub(crate) message_id: u64,
    pub(crate) staff_id: u64,
    pub(crate) at: i64,
}

/// False when the feat already links that post.
pub(crate) fn add_link(conn: &Connection, l: &NewLink) -> rusqlite::Result<bool> {
    let added = conn.execute(
        "INSERT INTO feat_links (feat_id, channel_id, message_id, added_by, added_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (feat_id, message_id) DO NOTHING",
        params![l.feat_id, to_sql(l.channel_id), to_sql(l.message_id), to_sql(l.staff_id), l.at],
    )?;
    Ok(added > 0)
}

pub(crate) fn remove_link(conn: &Connection, feat_id: i64, link_id: i64) -> rusqlite::Result<bool> {
    let removed = conn.execute("DELETE FROM feat_links WHERE feat_id = ?1 AND id = ?2", params![feat_id, link_id])?;
    Ok(removed > 0)
}
