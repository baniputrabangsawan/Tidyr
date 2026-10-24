//! Undo journal (SQLite): every action is recorded BEFORE execution
//! (write-ahead), then marked done. On startup an interrupted session can be
//! rolled back or finished.

use std::path::{Path, PathBuf};

use chrono::Local;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::plan::{Action, Category};
use crate::{Error, Result};

/// Session lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    InProgress,
    Done,
    Undone,
    RolledBack,
}

impl SessionStatus {
    fn as_str(self) -> &'static str {
        match self {
            SessionStatus::InProgress => "in_progress",
            SessionStatus::Done => "done",
            SessionStatus::Undone => "undone",
            SessionStatus::RolledBack => "rolled_back",
        }
    }
    fn from_str(s: &str) -> Self {
        match s {
            "done" => SessionStatus::Done,
            "undone" => SessionStatus::Undone,
            "rolled_back" => SessionStatus::RolledBack,
            _ => SessionStatus::InProgress,
        }
    }
}

/// One journaled action entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: i64,
    pub action: Action,
    pub category: Category,
    /// Inverse data needed for undo (e.g. quarantine record id).
    pub undo_data: serde_json::Value,
    pub done: bool,
}

/// A session shown on the "Riwayat" screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub started_at: String,
    pub status: SessionStatus,
    pub files_moved: usize,
    pub files_quarantined: usize,
    pub label: String,
}

/// Open (creating if needed) the journal database.
pub fn open(db_path: &Path) -> Result<Connection> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(db_path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            started_at TEXT NOT NULL,
            status TEXT NOT NULL,
            files_moved INTEGER NOT NULL DEFAULT 0,
            files_quarantined INTEGER NOT NULL DEFAULT 0,
            label TEXT NOT NULL DEFAULT ''
        );
        CREATE TABLE IF NOT EXISTS entries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            action TEXT NOT NULL,
            category TEXT NOT NULL,
            undo_data TEXT NOT NULL DEFAULT '{}',
            done INTEGER NOT NULL DEFAULT 0
        );",
    )?;
    Ok(conn)
}

/// Start a session; returns the session id.
pub fn begin_session(conn: &Connection, label: &str) -> Result<String> {
    let id = format!("{}-{}", Local::now().format("%Y%m%d-%H%M%S"), rand_suffix());
    conn.execute(
        "INSERT INTO sessions (id, started_at, status, label) VALUES (?1, ?2, 'in_progress', ?3)",
        params![id, Local::now().to_rfc3339(), label],
    )?;
    Ok(id)
}

/// Record an action BEFORE executing it. Returns the entry id.
pub fn record_before(
    conn: &Connection,
    session_id: &str,
    action: &Action,
    category: Category,
    undo_data: &serde_json::Value,
) -> Result<i64> {
    let action_json = serde_json::to_string(action)?;
    let cat = serde_json::to_string(&category)?;
    let undo = undo_data.to_string();
    conn.execute(
        "INSERT INTO entries (session_id, action, category, undo_data) VALUES (?1, ?2, ?3, ?4)",
        params![session_id, action_json, cat, undo],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Mark an entry done AFTER successful execution.
pub fn mark_done(conn: &Connection, entry_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE entries SET done = 1 WHERE id = ?1",
        params![entry_id],
    )?;
    Ok(())
}

/// Attach undo data (e.g. quarantine record id) to a specific entry.
/// Always addressed by entry id — never by LIKE matching, which could hit
/// several entries with the same file name in different folders.
pub fn set_undo_data(
    conn: &Connection,
    entry_id: i64,
    undo_data: &serde_json::Value,
) -> Result<()> {
    conn.execute(
        "UPDATE entries SET undo_data = ?1 WHERE id = ?2",
        params![undo_data.to_string(), entry_id],
    )?;
    Ok(())
}

/// Finish a session with counts for the Riwayat screen.
pub fn finish_session(
    conn: &Connection,
    session_id: &str,
    files_moved: usize,
    files_quarantined: usize,
) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET status = 'done', files_moved = ?1, files_quarantined = ?2 WHERE id = ?3",
        params![files_moved as i64, files_quarantined as i64, session_id],
    )?;
    Ok(())
}

/// Mark a session undone / rolled back.
pub fn set_session_status(
    conn: &Connection,
    session_id: &str,
    status: SessionStatus,
) -> Result<()> {
    conn.execute(
        "UPDATE sessions SET status = ?1 WHERE id = ?2",
        params![status.as_str(), session_id],
    )?;
    Ok(())
}

/// Sessions that were interrupted mid-apply (crash recovery).
pub fn interrupted_sessions(conn: &Connection) -> Result<Vec<SessionInfo>> {
    list_sessions_with(conn, "WHERE status = 'in_progress'")
}

/// All sessions, newest first.
pub fn list_sessions(conn: &Connection) -> Result<Vec<SessionInfo>> {
    list_sessions_with(conn, "ORDER BY started_at DESC")
}

fn list_sessions_with(conn: &Connection, clause: &str) -> Result<Vec<SessionInfo>> {
    let sql = format!(
        "SELECT id, started_at, status, files_moved, files_quarantined, label FROM sessions {clause}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(SessionInfo {
            id: row.get(0)?,
            started_at: row.get(1)?,
            status: SessionStatus::from_str(&row.get::<_, String>(2)?),
            files_moved: row.get::<_, i64>(3)? as usize,
            files_quarantined: row.get::<_, i64>(4)? as usize,
            label: row.get(5)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Entries of a session in execution order (for undo: reverse this).
pub fn session_entries(conn: &Connection, session_id: &str) -> Result<Vec<JournalEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, action, category, undo_data, done FROM entries WHERE session_id = ?1 ORDER BY id ASC",
    )?;
    let rows = stmt.query_map(params![session_id], |row| {
        let action_json: String = row.get(1)?;
        let cat_json: String = row.get(2)?;
        let undo_json: String = row.get(3)?;
        Ok(JournalEntry {
            id: row.get(0)?,
            action: serde_json::from_str(&action_json).unwrap_or(Action::DeleteEmptyDir {
                path: PathBuf::new(),
            }),
            category: serde_json::from_str(&cat_json).unwrap_or(Category::Rapikan),
            undo_data: serde_json::from_str(&undo_json).unwrap_or(serde_json::json!({})),
            done: row.get::<_, i64>(4)? != 0,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(Error::Journal)?);
    }
    Ok(out)
}

fn rand_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{n:08x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_ahead_and_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(&dir.path().join("journal.db")).unwrap();
        let sid = begin_session(&conn, "test").unwrap();
        // Recorded but never marked done -> simulates a crash mid-apply.
        let action = Action::Move {
            from: PathBuf::from("/a"),
            to: PathBuf::from("/b"),
        };
        record_before(
            &conn,
            &sid,
            &action,
            Category::Rapikan,
            &serde_json::json!({}),
        )
        .unwrap();
        let interrupted = interrupted_sessions(&conn).unwrap();
        assert_eq!(interrupted.len(), 1);
        assert_eq!(interrupted[0].id, sid);
        // Roll back -> no longer interrupted.
        set_session_status(&conn, &sid, SessionStatus::RolledBack).unwrap();
        assert!(interrupted_sessions(&conn).unwrap().is_empty());
    }

    #[test]
    fn entries_roundtrip_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(&dir.path().join("j.db")).unwrap();
        let sid = begin_session(&conn, "t").unwrap();
        for i in 0..3 {
            let a = Action::DeleteEmptyDir {
                path: PathBuf::from(format!("/d/{i}")),
            };
            let id = record_before(
                &conn,
                &sid,
                &a,
                Category::FolderKosong,
                &serde_json::json!({}),
            )
            .unwrap();
            mark_done(&conn, id).unwrap();
        }
        let entries = session_entries(&conn, &sid).unwrap();
        assert_eq!(entries.len(), 3);
        assert!(entries.iter().all(|e| e.done));
        finish_session(&conn, &sid, 0, 0).unwrap();
        let all = list_sessions(&conn).unwrap();
        assert_eq!(all[0].status, SessionStatus::Done);
    }
}
