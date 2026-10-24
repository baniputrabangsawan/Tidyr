//! Duplicate detection pipeline (exact duplicates only in the MVP).
//!
//! Staged to avoid hashing everything:
//! 1. Group by file size (ignore unique sizes; 0-byte files never reach here).
//! 2. Group by BLAKE3 of first 64 KB (+ last 64 KB for files > 1 MB).
//! 3. Full BLAKE3 hash for remaining candidates, in parallel via rayon.
//! 4. Optional byte-by-byte verification before any deletion proposal.
//!
//! Hardlinks (same file id) are NOT treated as space-saving duplicates.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use rusqlite::Connection;

use crate::cache;
use crate::scan::FileEntry;

pub const PARTIAL_LEN: u64 = 64 * 1024;
pub const LARGE_FILE_THRESHOLD: u64 = 1024 * 1024;

/// One group of files with identical content.
#[derive(Debug, Clone)]
pub struct DuplicateGroup {
    pub hash: String,
    pub files: Vec<FileEntry>,
    /// True when all members are hardlinks to the same inode (no space saved).
    pub is_hardlink_set: bool,
    /// Bytes that quarantining all-but-one would free.
    pub reclaimable_bytes: u64,
}

/// Staged duplicate detection over `files`. Uses `cache_conn` when given.
pub fn find_duplicates(
    files: &[FileEntry],
    cache_conn: Option<&Connection>,
) -> Vec<DuplicateGroup> {
    // Stage 1: group by size.
    let mut by_size: HashMap<u64, Vec<&FileEntry>> = HashMap::new();
    for f in files {
        by_size.entry(f.size).or_default().push(f);
    }
    let candidates: Vec<&FileEntry> = by_size
        .values()
        .filter(|g| g.len() > 1)
        .flat_map(|g| g.iter().copied())
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }

    // Stage 2: partial hash.
    let mut by_partial: HashMap<String, Vec<&FileEntry>> = HashMap::new();
    for f in &candidates {
        match partial_hash(&f.path, f.size, f.mtime, cache_conn) {
            Some(h) => {
                by_partial.entry(h).or_default().push(*f);
            }
            None => {
                // Unreadable now: skip (scan already records unreadable files).
            }
        }
    }

    // Stage 3: full hash in parallel. The SQLite cache handle is not Sync,
    // so the parallel pass hashes without cache; results are merged into
    // the cache serially afterwards (write-ahead of stage 4).
    let mut groups = Vec::new();
    for members in by_partial.values().filter(|g| g.len() > 1) {
        let hashed: Vec<(&FileEntry, Option<String>)> = members
            .par_iter()
            .map(|f| (*f, full_hash_nocache(&f.path)))
            .collect();
        if let Some(conn) = cache_conn {
            for (f, h) in &hashed {
                if let Some(h) = h {
                    let existing_partial =
                        cache::get_partial(conn, &f.path, f.size, f.mtime).unwrap_or_default();
                    let _ = cache::put(conn, &f.path, f.size, f.mtime, h, &existing_partial);
                }
            }
        }
        let mut by_full: HashMap<String, Vec<&FileEntry>> = HashMap::new();
        for (f, h) in hashed {
            if let Some(h) = h {
                by_full.entry(h).or_default().push(f);
            }
        }
        for (hash, fs) in by_full.into_iter().filter(|(_, g)| g.len() > 1) {
            // Stage 4: byte-by-byte verification for small groups.
            let files: Vec<FileEntry> = fs.into_iter().cloned().collect();
            if files.len() <= 8 && !verify_byte_equal(&files) {
                continue;
            }
            let is_hardlink_set = is_single_inode(&files);
            let reclaimable_bytes = if is_hardlink_set {
                0
            } else {
                files.iter().map(|f| f.size).sum::<u64>()
                    - files.iter().map(|f| f.size).min().unwrap_or(0)
            };
            groups.push(DuplicateGroup {
                hash,
                files,
                is_hardlink_set,
                reclaimable_bytes,
            });
        }
    }
    // Deterministic order for tests and UI.
    groups.sort_by(|a, b| {
        b.reclaimable_bytes
            .cmp(&a.reclaimable_bytes)
            .then(a.hash.cmp(&b.hash))
    });
    groups
}

/// BLAKE3 of first 64 KB (+ last 64 KB when size > 1 MB).
pub fn partial_hash(
    path: &Path,
    size: u64,
    mtime: i64,
    cache_conn: Option<&Connection>,
) -> Option<String> {
    if let Some(conn) = cache_conn {
        if let Some(h) = cache::get_partial(conn, path, size, mtime) {
            return Some(h);
        }
    }
    let mut file = File::open(path).ok()?;
    let mut hasher = blake3::Hasher::new();
    let head = PARTIAL_LEN.min(size);
    let mut buf = vec![0u8; head as usize];
    file.read_exact(&mut buf).ok()?;
    hasher.update(&buf);
    if size > LARGE_FILE_THRESHOLD {
        file.seek(SeekFrom::End(-(PARTIAL_LEN as i64))).ok()?;
        let mut tail = vec![0u8; PARTIAL_LEN as usize];
        file.read_exact(&mut tail).ok()?;
        hasher.update(&tail);
    }
    let hex = hasher.finalize().to_hex().to_string();
    if let Some(conn) = cache_conn {
        // Cache the partial hash; full hash filled in later by full_hash().
        let existing_full = cache::get_full(conn, path, size, mtime).unwrap_or_default();
        let _ = cache::put(conn, path, size, mtime, &existing_full, &hex);
    }
    Some(hex)
}

/// Full BLAKE3 hash, streamed (never holds whole file in memory).
pub fn full_hash(
    path: &Path,
    size: u64,
    mtime: i64,
    cache_conn: Option<&Connection>,
) -> Option<String> {
    if let Some(conn) = cache_conn {
        if let Some(h) = cache::get_full(conn, path, size, mtime) {
            if !h.is_empty() {
                return Some(h);
            }
        }
    }
    let mut file = File::open(path).ok()?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let hex = hasher.finalize().to_hex().to_string();
    if let Some(conn) = cache_conn {
        let existing_partial = cache::get_partial(conn, path, size, mtime).unwrap_or_default();
        let _ = cache::put(conn, path, size, mtime, &hex, &existing_partial);
    }
    Some(hex)
}

/// Full BLAKE3 hash without touching the cache (for use in `par_iter`,
/// since the SQLite handle is not `Sync`). Prefer [`full_hash`].
pub fn full_hash_nocache(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(hasher.finalize().to_hex().to_string())
}

/// Byte-by-byte verification that all files have identical content.
pub fn verify_byte_equal(files: &[FileEntry]) -> bool {
    if files.is_empty() {
        return true;
    }
    let first = &files[0];
    for other in &files[1..] {
        if other.size != first.size || !files_byte_equal(&first.path, &other.path) {
            return false;
        }
    }
    true
}

fn files_byte_equal(a: &Path, b: &Path) -> bool {
    let (mut fa, mut fb) = match (File::open(a), File::open(b)) {
        (Ok(fa), Ok(fb)) => (fa, fb),
        _ => return false,
    };
    let mut ba = vec![0u8; 256 * 1024];
    let mut bb = vec![0u8; 256 * 1024];
    loop {
        let (na, nb) = match (fa.read(&mut ba), fb.read(&mut bb)) {
            (Ok(na), Ok(nb)) => (na, nb),
            _ => return false,
        };
        if na != nb || ba[..na] != bb[..nb] {
            return false;
        }
        if na == 0 {
            return true;
        }
    }
}

/// True when every member shares one inode/file id (hardlink set).
fn is_single_inode(files: &[FileEntry]) -> bool {
    let mut ids: Vec<&str> = files.iter().filter_map(|f| f.file_id.as_deref()).collect();
    if ids.len() != files.len() || ids.is_empty() {
        return false;
    }
    ids.sort_unstable();
    ids.dedup();
    ids.len() == 1
}

/// Re-check that a file still matches its scanned (size, mtime).
pub fn unchanged_since_scan(path: &Path, size: u64, mtime: i64) -> bool {
    match std::fs::metadata(path) {
        Ok(m) => {
            if m.len() != size {
                return false;
            }
            let mt = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(-1);
            mt == mtime
        }
        Err(_) => false,
    }
}

pub fn fixture_entry(path: PathBuf, size: u64) -> FileEntry {
    FileEntry {
        path,
        size,
        mtime: 0,
        is_readonly: false,
        file_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn staged_pipeline_finds_exact_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.bin");
        let b = dir.path().join("b.bin");
        let c = dir.path().join("different.bin");
        fs::write(&a, b"same-content-here!!").unwrap();
        fs::write(&b, b"same-content-here!!").unwrap();
        fs::write(&c, b"same-content-here!?").unwrap();
        let files = vec![
            fixture_entry(a, 19),
            fixture_entry(b, 19),
            fixture_entry(c, 19),
        ];
        let groups = find_duplicates(&files, None);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].files.len(), 2);
        assert!(groups[0].reclaimable_bytes > 0);
    }

    #[test]
    fn unique_sizes_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.bin");
        fs::write(&a, b"abc").unwrap();
        let files = vec![fixture_entry(a, 3)];
        assert!(find_duplicates(&files, None).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn hardlinks_are_not_space_saving() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.bin");
        fs::write(&a, b"hardlink-content-1234").unwrap();
        let b = dir.path().join("b.bin");
        fs::hard_link(&a, &b).unwrap();
        // Re-scan metadata so file_ids are populated like the real scanner does.
        let entry_of = |p: PathBuf| {
            let m = std::fs::metadata(&p).unwrap();
            use std::os::unix::fs::MetadataExt;
            FileEntry {
                size: m.len(),
                mtime: 0,
                is_readonly: false,
                file_id: Some(format!("{}:{}", m.dev(), m.ino())),
                path: p,
            }
        };
        let groups = find_duplicates(&[entry_of(a), entry_of(b)], None);
        assert_eq!(groups.len(), 1);
        assert!(groups[0].is_hardlink_set);
        assert_eq!(groups[0].reclaimable_bytes, 0);
    }

    #[test]
    fn changed_since_scan_detected() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.txt");
        fs::write(&p, b"12345").unwrap();
        let m = std::fs::metadata(&p).unwrap();
        let mt = m
            .modified()
            .unwrap()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert!(unchanged_since_scan(&p, 5, mt));
        fs::write(&p, b"123456789").unwrap();
        assert!(!unchanged_since_scan(&p, 5, mt));
    }
}
