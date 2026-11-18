//! Characters and the one each player plays now.

use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{from_sql, to_sql};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CharacterRow {
    pub(crate) id: i64,
    pub(crate) player_id: u64,
    pub(crate) slot: i64,
    pub(crate) name: String,
    pub(crate) race: Option<String>,
    pub(crate) tier: Option<i64>,
    pub(crate) title: Option<String>,
    pub(crate) bio: Option<String>,
    /// None: the owner's Discord profile icon.
    pub(crate) picture_url: Option<String>,
    /// Some: an uploaded picture, this message in the pictures channel; picture_url is its last link.
    pub(crate) picture_message: Option<u64>,
    /// For posting as them; NPCs only, for now.
    pub(crate) prefix: Option<String>,
    /// Staff only, never on a card.
    pub(crate) notes: Option<String>,
    /// Whether "prefix: text" posts as them.
    pub(crate) posting: bool,
    /// Roleplay posts made as them.
    pub(crate) posts: i64,
}

const COLUMNS: &str = "id, player_id, slot, name, race, tier, title, bio, picture_url, prefix, notes, picture_message, posting, posts";

fn row_of(row: &Row) -> rusqlite::Result<CharacterRow> {
    Ok(CharacterRow {
        id: row.get(0)?,
        player_id: from_sql(row.get(1)?),
        slot: row.get(2)?,
        name: row.get(3)?,
        race: row.get(4)?,
        tier: row.get(5)?,
        title: row.get(6)?,
        bio: row.get(7)?,
        picture_url: row.get(8)?,
        prefix: row.get(9)?,
        notes: row.get(10)?,
        picture_message: row.get::<_, Option<i64>>(11)?.map(from_sql),
        posting: row.get(12)?,
        posts: row.get(13)?,
    })
}

pub(crate) struct NewCharacter<'a> {
    pub(crate) player_id: u64,
    pub(crate) slot: i64,
    pub(crate) name: &'a str,
    pub(crate) staff_id: u64,
    pub(crate) at: i64,
}

/// `None` when the slot is taken.
pub(crate) fn insert(conn: &Connection, c: &NewCharacter) -> rusqlite::Result<Option<i64>> {
    let added = conn.execute(
        "INSERT INTO characters (player_id, slot, name, added_by, added_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (player_id, slot) DO NOTHING",
        params![to_sql(c.player_id), c.slot, c.name, to_sql(c.staff_id), c.at],
    )?;
    Ok((added > 0).then(|| conn.last_insert_rowid()))
}

pub(crate) fn get(conn: &Connection, id: i64) -> rusqlite::Result<Option<CharacterRow>> {
    conn.query_row(&format!("SELECT {COLUMNS} FROM characters WHERE id = ?1"), [id], row_of).optional()
}

/// Slot order.
pub(crate) fn of_player(conn: &Connection, player_id: u64) -> rusqlite::Result<Vec<CharacterRow>> {
    let mut stmt = conn.prepare_cached(&format!("SELECT {COLUMNS} FROM characters WHERE player_id = ?1 ORDER BY slot"))?;
    stmt.query_map([to_sql(player_id)], row_of)?.collect()
}

pub(crate) fn active(conn: &Connection, player_id: u64) -> rusqlite::Result<Option<CharacterRow>> {
    conn.query_row(
        "SELECT c.id, c.player_id, c.slot, c.name, c.race, c.tier, c.title, c.bio, c.picture_url, c.prefix, c.notes, c.picture_message, c.posting, c.posts
         FROM active_characters a JOIN characters c ON c.id = a.character_id WHERE a.player_id = ?1",
        [to_sql(player_id)],
        row_of,
    )
    .optional()
}

/// Every character, players' and the cast's, by owner then slot.
pub(crate) fn all(conn: &Connection) -> rusqlite::Result<Vec<CharacterRow>> {
    let mut stmt = conn.prepare_cached(&format!("SELECT {COLUMNS} FROM characters ORDER BY player_id, slot"))?;
    stmt.query_map([], row_of)?.collect()
}

/// The next free slot of a player: NPCs have no slot limit.
pub(crate) fn next_slot(conn: &Connection, player_id: u64) -> rusqlite::Result<i64> {
    conn.query_row("SELECT ifnull(max(slot), 0) + 1 FROM characters WHERE player_id = ?1", [to_sql(player_id)], |row| row.get(0))
}

/// A player's character using a prefix, any case; the cast is player 0.
pub(crate) fn by_prefix(conn: &Connection, player_id: u64, prefix: &str) -> rusqlite::Result<Option<CharacterRow>> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM characters WHERE player_id = ?1 AND lower(prefix) = lower(?2)"),
        params![to_sql(player_id), prefix],
        row_of,
    )
    .optional()
}

/// A prefix and whether it posts, set by the character's player.
pub(crate) fn set_posting(conn: &Connection, id: i64, prefix: Option<&str>, posting: bool) -> rusqlite::Result<()> {
    conn.execute("UPDATE characters SET prefix = ?2, posting = ?3 WHERE id = ?1", params![id, prefix, posting])?;
    Ok(())
}

/// One more roleplay post.
pub(crate) fn count_post(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE characters SET posts = posts + 1 WHERE id = ?1", [id])?;
    Ok(())
}

/// What a sheet edits; every field is written as given.
pub(crate) struct SheetFields<'a> {
    pub(crate) name: &'a str,
    pub(crate) prefix: Option<&'a str>,
    pub(crate) race: Option<&'a str>,
    pub(crate) tier: Option<i64>,
    pub(crate) title: Option<&'a str>,
    pub(crate) bio: Option<&'a str>,
    pub(crate) picture_url: Option<&'a str>,
    pub(crate) picture_message: Option<u64>,
    pub(crate) notes: Option<&'a str>,
    pub(crate) posting: bool,
}

pub(crate) fn save_sheet(conn: &Connection, id: i64, f: &SheetFields) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE characters SET name = ?2, prefix = ?3, race = ?4, tier = ?5, title = ?6, bio = ?7, picture_url = ?8, notes = ?9,
         picture_message = ?10, posting = ?11 WHERE id = ?1",
        params![id, f.name, f.prefix, f.race, f.tier, f.title, f.bio, f.picture_url, f.notes, f.picture_message.map(to_sql), f.posting],
    )?;
    Ok(())
}

/// An uploaded picture: its message in the pictures channel, and the link it had then. False: no such character.
pub(crate) fn set_picture(conn: &Connection, id: i64, url: &str, message_id: u64) -> rusqlite::Result<bool> {
    let changed = conn.execute("UPDATE characters SET picture_url = ?2, picture_message = ?3 WHERE id = ?1", params![id, url, to_sql(message_id)])?;
    Ok(changed > 0)
}

pub(crate) fn set_active(conn: &Connection, player_id: u64, character_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO active_characters (player_id, character_id) VALUES (?1, ?2)
         ON CONFLICT (player_id) DO UPDATE SET character_id = excluded.character_id",
        params![to_sql(player_id), character_id],
    )?;
    Ok(())
}
