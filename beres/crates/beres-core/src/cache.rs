//! Hash cache (SQLite): keyed by (path, size, mtime) so a second scan is fast.
//!
//! Stores full BLAKE3 hashes plus partial (first/last 64 KB) hashes.

use std::path::Path;

use rusqlite::{params, Connection};

use crate::Result;

/// Open (creating if needed) the cache database at `db_path`.
pub fn open(db_path: &Path) -> Result<Connection> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(db_path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS hashes (
            path TEXT NOT NULL,
            size INTEGER NOT NULL,
            mtime INTEGER NOT NULL,
            full_hash TEXT NOT NULL,
            partial_hash TEXT NOT NULL DEFAULT '',
            PRIMARY KEY (path, size, mtime)
        );",
    )?;
    // Migration for DBs created before partial_hash existed.
    let _ = conn.execute(
        "ALTER TABLE hashes ADD COLUMN partial_hash TEXT NOT NULL DEFAULT ''",
        [],
    );
    Ok(conn)
}

/// Look up a cached full hash. Returns None on any miss.
pub fn get_full(conn: &Connection, path: &Path, size: u64, mtime: i64) -> Option<String> {
    conn.query_row(
        "SELECT full_hash FROM hashes WHERE path = ?1 AND size = ?2 AND mtime = ?3",
        params![path.to_string_lossy(), size as i64, mtime],
        |row| row.get::<_, String>(0),
    )
    .ok()
}

/// Look up a cached partial hash. Returns None on any miss.
pub fn get_partial(conn: &Connection, path: &Path, size: u64, mtime: i64) -> Option<String> {
    let s: String = conn
        .query_row(
            "SELECT partial_hash FROM hashes WHERE path = ?1 AND size = ?2 AND mtime = ?3",
            params![path.to_string_lossy(), size as i64, mtime],
            |row| row.get(0),
        )
        .ok()?;
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Insert or replace a hash record.
pub fn put(
    conn: &Connection,
    path: &Path,
    size: u64,
    mtime: i64,
    full_hash: &str,
    partial_hash: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO hashes (path, size, mtime, full_hash, partial_hash)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            path.to_string_lossy(),
            size as i64,
            mtime,
            full_hash,
            partial_hash
        ],
    )?;
    Ok(())
}

/// Validate the DB handle is usable; maps rusqlite errors into [`Error`].
pub fn ping(conn: &Connection) -> Result<()> {
    conn.query_row("SELECT 1", [], |_| Ok(()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_mtime_invalidation() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("cache.db");
        let conn = open(&db).unwrap();
        let p = Path::new("/x/a.txt");
        assert!(get_full(&conn, p, 5, 100).is_none());
        put(&conn, p, 5, 100, "abc", "par").unwrap();
        assert_eq!(get_full(&conn, p, 5, 100).as_deref(), Some("abc"));
        assert_eq!(get_partial(&conn, p, 5, 100).as_deref(), Some("par"));
        // Changed mtime invalidates.
        assert!(get_full(&conn, p, 5, 101).is_none());
        ping(&conn).unwrap();
    }
}
