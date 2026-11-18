//! Where each person is in the interactive tutorial.

use rusqlite::{Connection, OptionalExtension, params};

use super::{from_sql, to_sql};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TutorialRow {
    pub(crate) user_id: u64,
    pub(crate) track: String,
    pub(crate) step: String,
    pub(crate) thread_id: Option<u64>,
}

pub(crate) fn get(conn: &Connection, user_id: u64) -> rusqlite::Result<Option<TutorialRow>> {
    conn.query_row("SELECT user_id, track, step, thread_id FROM tutorials WHERE user_id = ?1", [to_sql(user_id)], |row| {
        Ok(TutorialRow { user_id: from_sql(row.get(0)?), track: row.get(1)?, step: row.get(2)?, thread_id: row.get::<_, Option<i64>>(3)?.map(from_sql) })
    })
    .optional()
}

/// Starts or restarts a tutorial.
pub(crate) fn start(conn: &Connection, t: &TutorialRow, at: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO tutorials (user_id, track, step, thread_id, started_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (user_id) DO UPDATE SET track = excluded.track, step = excluded.step, thread_id = excluded.thread_id, started_at = excluded.started_at",
        params![to_sql(t.user_id), t.track, t.step, t.thread_id.map(to_sql), at],
    )?;
    Ok(())
}

pub(crate) fn set_step(conn: &Connection, user_id: u64, step: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE tutorials SET step = ?2 WHERE user_id = ?1", params![to_sql(user_id), step])?;
    Ok(())
}

pub(crate) fn stop(conn: &Connection, user_id: u64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM tutorials WHERE user_id = ?1", [to_sql(user_id)])?;
    Ok(())
}
