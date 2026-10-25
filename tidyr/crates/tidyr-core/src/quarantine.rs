//! App-managed quarantine: nothing is permanently deleted except purge
//! after the retention period (default 30 days).
//!
//! Files are moved here (rename on the same volume, copy+verify+delete
//! across volumes). Each record stores the original path, hash, size,
//! timestamps, and session id so it can be restored.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// One quarantined file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuarantineRecord {
    pub id: i64,
    pub original_path: PathBuf,
    pub stored_path: PathBuf,
    pub size: u64,
    pub mtime: i64,
    pub hash: String,
    pub session_id: String,
    pub quarantined_at: i64,
}
/// Summary report returned when expired quarantine records are purged.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurgeReport {
    pub count: usize,
    pub bytes: u64,
    pub purged_paths: Vec<PathBuf>,
}

/// Outcome of restoring a quarantined file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreOutcome {
    Restored(PathBuf),
    RestoredWithSuffix(PathBuf),
}

impl RestoreOutcome {
    pub fn path(&self) -> &Path {
        match self {
            RestoreOutcome::Restored(p) | RestoreOutcome::RestoredWithSuffix(p) => p,
        }
    }
}

pub struct Quarantine {
    dir: PathBuf,
    conn: Connection,
}

impl Quarantine {
    /// Open (creating if needed) the quarantine store inside `app_data_dir`.
    pub fn open(app_data_dir: &Path) -> Result<Self> {
        let dir = app_data_dir.join("quarantine");
        fs::create_dir_all(dir.join("files"))?;
        let conn = Connection::open(dir.join("quarantine.db"))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                original_path TEXT NOT NULL,
                stored_path TEXT NOT NULL,
                size INTEGER NOT NULL,
                mtime INTEGER NOT NULL,
                hash TEXT NOT NULL,
                session_id TEXT NOT NULL,
                quarantined_at INTEGER NOT NULL
            );",
        )?;
        Ok(Self { dir, conn })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Move `path` into quarantine. Returns the stored record.
    pub fn quarantine_file(
        &self,
        path: &Path,
        size: u64,
        mtime: i64,
        session_id: &str,
    ) -> Result<QuarantineRecord> {
        let meta = fs::metadata(path).map_err(|_| Error::ChangedSinceScan(path.to_path_buf()))?;
        if meta.len() != size {
            return Err(Error::ChangedSinceScan(path.to_path_buf()));
        }
        let hash = blake3_file(path)?;
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let stored = self.unique_stored_name(file_name)?;
        move_file(path, &stored)?;
        let now = now_secs();
        self.conn.execute(
            "INSERT INTO records
             (original_path, stored_path, size, mtime, hash, session_id, quarantined_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                path.to_string_lossy(),
                stored.to_string_lossy(),
                size as i64,
                mtime,
                hash,
                session_id,
                now
            ],
        )?;
        Ok(QuarantineRecord {
            id: self.conn.last_insert_rowid(),
            original_path: path.to_path_buf(),
            stored_path: stored,
            size,
            mtime,
            hash,
            session_id: session_id.to_string(),
            quarantined_at: now,
        })
    }

    /// Restore one record by id. If the original path is occupied, restore
    /// with a suffix and report it (never overwrite).
    pub fn restore(&self, id: i64) -> Result<RestoreOutcome> {
        let rec = self
            .get(id)?
            .ok_or_else(|| Error::ActionFailed("quarantine-record-missing".to_string()))?;
        let outcome = self.restore_record(&rec)?;
        self.delete_record(id)?;
        Ok(outcome)
    }

    /// Restore using an already-loaded record (used by session undo).
    pub fn restore_record(&self, rec: &QuarantineRecord) -> Result<RestoreOutcome> {
        if !rec.stored_path.exists() {
            // Stored file is gone: if the original is back with the right
            // size, treat it as already restored; otherwise report loss.
            if fs::metadata(&rec.original_path)
                .map(|m| m.len())
                .unwrap_or(u64::MAX)
                == rec.size
            {
                return Ok(RestoreOutcome::Restored(rec.original_path.clone()));
            }
            return Err(Error::ActionFailed("quarantined-file-missing".to_string()));
        }
        let dest = if rec.original_path.exists() {
            let parent = rec.original_path.parent().unwrap_or_else(|| Path::new("."));
            let name = rec
                .original_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file");
            crate::rules::suffixed_destination(parent, name)
        } else {
            rec.original_path.clone()
        };
        let suffixed = dest != rec.original_path;
        move_file(&rec.stored_path, &dest)?;
        // Defense in depth: restored size must match the record.
        let back = fs::metadata(&dest).map(|m| m.len()).unwrap_or(u64::MAX);
        if back != rec.size {
            return Err(Error::ActionFailed("restore-size-mismatch".to_string()));
        }
        if suffixed {
            Ok(RestoreOutcome::RestoredWithSuffix(dest))
        } else {
            Ok(RestoreOutcome::Restored(dest))
        }
    }

    pub fn get(&self, id: i64) -> Result<Option<QuarantineRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, original_path, stored_path, size, mtime, hash, session_id, quarantined_at
             FROM records WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], row_to_record)?;
        match rows.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }

    /// All records, newest first (for the Riwayat / quarantine browser).
    pub fn list(&self) -> Result<Vec<QuarantineRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, original_path, stored_path, size, mtime, hash, session_id, quarantined_at
             FROM records ORDER BY quarantined_at DESC",
        )?;
        let rows = stmt.query_map([], row_to_record)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Permanently delete records older than `retention_days`. This is the
    /// ONLY place in the app where files are permanently deleted.
    /// Returns a PurgeReport with counts, bytes freed, and original paths.
    pub fn purge_expired(&self, retention_days: i64, now_secs: i64) -> Result<PurgeReport> {
        let cutoff = now_secs - retention_days * 86_400;
        let mut stmt = self.conn.prepare(
            "SELECT id, original_path, stored_path, size, mtime, hash, session_id, quarantined_at
             FROM records WHERE quarantined_at <= ?1",
        )?;
        let rows = stmt.query_map(params![cutoff], row_to_record)?;
        let mut expired = Vec::new();
        for r in rows {
            expired.push(r?);
        }
        let mut purged_paths = Vec::new();
        let mut bytes = 0u64;
        for rec in expired {
            match fs::remove_file(&rec.stored_path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(Error::Io(e)),
            }
            self.delete_record(rec.id)?;
            bytes += rec.size;
            purged_paths.push(rec.original_path);
        }
        Ok(PurgeReport {
            count: purged_paths.len(),
            bytes,
            purged_paths,
        })
    }

    fn delete_record(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM records WHERE id = ?1", params![id])?;
        Ok(())
    }

    fn unique_stored_name(&self, file_name: &str) -> Result<PathBuf> {
        let files_dir = self.dir.join("files");
        let (stem, ext) = match file_name.rfind('.') {
            Some(i) if i > 0 => (&file_name[..i], Some(&file_name[i + 1..])),
            _ => (file_name, None),
        };
        // Sanitize the stem so stored names are always safe.
        let clean: String = stem
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                    c
                } else {
                    '_'
                }
            })
            .take(60)
            .collect();
        for attempt in 0..1000u32 {
            let stamp = now_nanos();
            let candidate = match ext {
                Some(e) => format!("{clean}__{stamp:x}_{attempt}.{e}"),
                None => format!("{clean}__{stamp:x}_{attempt}"),
            };
            let p = files_dir.join(candidate);
            if !p.exists() {
                return Ok(p);
            }
        }
        Err(Error::ActionFailed("quarantine-no-free-name".to_string()))
    }
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<QuarantineRecord> {
    Ok(QuarantineRecord {
        id: row.get(0)?,
        original_path: PathBuf::from(row.get::<_, String>(1)?),
        stored_path: PathBuf::from(row.get::<_, String>(2)?),
        size: row.get::<_, i64>(3)? as u64,
        mtime: row.get(4)?,
        hash: row.get(5)?,
        session_id: row.get(6)?,
        quarantined_at: row.get(7)?,
    })
}

/// Move a file, same volume (rename) or across volumes (copy + verify +
/// delete). Never overwrites: the copy fallback opens the destination with
/// `create_new`, so a concurrent creation fails instead of being clobbered.
pub fn move_file(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    // Check before link/copy
    if to.exists() {
        return Err(Error::WouldOverwrite(to.to_path_buf()));
    }
    // Atomic no-replace hard_link first: fails with AlreadyExists if `to` exists
    // (closing the check-then-rename race condition on both Unix and Windows).
    match fs::hard_link(from, to) {
        Ok(()) => {
            // Clear read-only on the source if set, so remove_file succeeds
            if let Ok(meta) = fs::metadata(from) {
                if meta.permissions().readonly() {
                    let mut perms = meta.permissions();
                    #[allow(clippy::permissions_set_readonly_false)]
                    perms.set_readonly(false);
                    let _ = fs::set_permissions(from, perms);
                }
            }
            if let Err(e) = fs::remove_file(from) {
                let _ = fs::remove_file(to);
                return Err(Error::Io(e));
            }
            Ok(())
        }
        Err(link_err) => {
            if link_err.kind() == io::ErrorKind::AlreadyExists {
                return Err(Error::WouldOverwrite(to.to_path_buf()));
            }
            // Fallback for cross-volume moves or filesystems without hardlink support:
            // copy_verify_delete uses create_new(true), guaranteeing atomic no-replace.
            copy_verify_delete(from, to).map_err(|e| match e {
                Error::WouldOverwrite(_) => e,
                _ => Error::ActionFailed(format!("move-failed: {}", short_err(&e.to_string()))),
            })
        }
    }
}
fn copy_verify_delete(from: &Path, to: &Path) -> Result<()> {
    let src_meta = fs::metadata(from)?;
    let src_hash = blake3_file(from)?;
    // create_new: never overwrite an existing destination.
    let mut src = fs::File::open(from)?;
    let mut dst = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)
        .map_err(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                Error::WouldOverwrite(to.to_path_buf())
            } else {
                Error::Io(e)
            }
        })?;
    if let Err(e) = io::copy(&mut src, &mut dst) {
        drop(dst);
        let _ = fs::remove_file(to);
        return Err(Error::Io(e));
    }
    drop(dst);
    let dst_hash = blake3_file(to)?;
    if dst_hash != src_hash {
        let _ = fs::remove_file(to);
        return Err(Error::ActionFailed("copy-verify-mismatch".to_string()));
    }
    // Preserve timestamps (rename would have preserved them).
    let mtime = filetime::FileTime::from_last_modification_time(&src_meta);
    let atime = filetime::FileTime::from_last_access_time(&src_meta);
    let _ = filetime::set_file_times(to, atime, mtime);

    // Handle read-only source: clear read-only on the source before deleting it
    if src_meta.permissions().readonly() {
        let mut perms = src_meta.permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        let _ = fs::set_permissions(from, perms);
    }
    if let Err(e) = fs::remove_file(from) {
        let _ = fs::remove_file(to);
        return Err(Error::Io(e));
    }
    Ok(())
}

/// Streaming BLAKE3 of a whole file (never holds it all in memory).
pub fn blake3_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = io::Read::read(&mut file, &mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

fn short_err(msg: &str) -> String {
    msg.chars().take(120).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup() -> (tempfile::TempDir, Quarantine) {
        let dir = tempfile::tempdir().unwrap();
        let q = Quarantine::open(&dir.path().join("appdata")).unwrap();
        (dir, q)
    }

    #[test]
    fn quarantine_and_restore_roundtrip() {
        let (dir, q) = setup();
        let f = dir.path().join("doc.txt");
        fs::write(&f, b"important bytes").unwrap();
        let meta = fs::metadata(&f).unwrap();
        let rec = q.quarantine_file(&f, meta.len(), 123, "s1").unwrap();
        assert!(!f.exists());
        assert_eq!(q.list().unwrap().len(), 1);
        let outcome = q.restore(rec.id).unwrap();
        assert_eq!(outcome, RestoreOutcome::Restored(f.clone()));
        assert_eq!(fs::read(&f).unwrap(), b"important bytes");
        assert!(q.list().unwrap().is_empty());
    }

    #[test]
    fn restore_with_suffix_when_occupied() {
        let (dir, q) = setup();
        let f = dir.path().join("doc.txt");
        fs::write(&f, b"v1").unwrap();
        let meta = fs::metadata(&f).unwrap();
        let rec = q.quarantine_file(&f, meta.len(), 1, "s1").unwrap();
        // Something else now occupies the original path.
        fs::write(&f, b"new occupant").unwrap();
        let outcome = q.restore(rec.id).unwrap();
        match outcome {
            RestoreOutcome::RestoredWithSuffix(p) => {
                assert_eq!(fs::read(&p).unwrap(), b"v1");
                assert_eq!(fs::read(&f).unwrap(), b"new occupant");
            }
            RestoreOutcome::Restored(_) => panic!("expected suffixed restore"),
        }
    }

    #[test]
    fn purge_by_retention_only_deletes_expired() {
        let (dir, q) = setup();
        let f = dir.path().join("old.txt");
        fs::write(&f, b"old").unwrap();
        let meta = fs::metadata(&f).unwrap();
        let rec = q.quarantine_file(&f, meta.len(), 1, "s1").unwrap();
        // Not yet expired.
        let report = q
            .purge_expired(30, rec.quarantined_at + 29 * 86_400)
            .unwrap();
        assert_eq!(report.count, 0);
        assert_eq!(report.bytes, 0);
        assert!(rec.stored_path.exists());
        // Expired now.
        let report = q
            .purge_expired(30, rec.quarantined_at + 31 * 86_400)
            .unwrap();
        assert_eq!(report.count, 1);
        assert_eq!(report.bytes, 3);
        assert_eq!(report.purged_paths.len(), 1);
        assert!(!rec.stored_path.exists());
        assert!(q.list().unwrap().is_empty());
    }

    #[test]
    fn move_never_overwrites() {
        let (dir, _q) = setup();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"A").unwrap();
        fs::write(&b, b"B").unwrap();
        let err = move_file(&a, &b).unwrap_err();
        assert!(matches!(err, Error::WouldOverwrite(_)));
        assert_eq!(fs::read(&b).unwrap(), b"B");
    }

    #[test]
    fn move_file_race_detected_when_destination_created() {
        let (dir, _q) = setup();
        let a = dir.path().join("source.txt");
        let b = dir.path().join("dest.txt");
        fs::write(&a, b"Source data").unwrap();
        // Destination created right before move
        fs::write(&b, b"Pre-existing destination").unwrap();
        let err = move_file(&a, &b).unwrap_err();
        assert!(matches!(err, Error::WouldOverwrite(_)));
        assert_eq!(fs::read(&a).unwrap(), b"Source data");
        assert_eq!(fs::read(&b).unwrap(), b"Pre-existing destination");
    }

    #[test]
    fn read_only_source_handled_cleanly() {
        let (dir, _q) = setup();
        let a = dir.path().join("readonly_source.txt");
        let b = dir.path().join("dest_from_ro.txt");
        fs::write(&a, b"Read only source data").unwrap();
        let mut perms = fs::metadata(&a).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&a, perms).unwrap();
        assert!(fs::metadata(&a).unwrap().permissions().readonly());

        // move_file must clear read-only on source and move to destination cleanly
        move_file(&a, &b).unwrap();
        assert!(!a.exists());
        assert!(b.exists());
        assert_eq!(fs::read(&b).unwrap(), b"Read only source data");
    }
}
