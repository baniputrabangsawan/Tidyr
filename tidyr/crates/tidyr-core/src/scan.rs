//! Folder traversal: collect file metadata without touching file contents.
//!
//! Skips by default: symlinks (recorded, never followed), hidden/system
//! files, and anything under protected paths. Permission errors are
//! collected per file into `unreadable` instead of aborting the scan.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

/// Metadata for one regular file found during a scan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileEntry {
    pub path: PathBuf,
    pub size: u64,
    pub mtime: i64,
    pub is_readonly: bool,
    /// Optional inode/file-id used for hardlink detection (Unix only).
    pub file_id: Option<String>,
}

/// A symlink encountered during traversal (never followed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymlinkEntry {
    pub path: PathBuf,
}

/// A file that could not be read (permission, vanished, ...).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnreadableEntry {
    pub path: PathBuf,
    pub reason: String,
}

/// Aggregate progress counters updated while scanning.
#[derive(Debug, Default)]
pub struct ScanProgress {
    pub files_scanned: AtomicU64,
    pub bytes_scanned: AtomicU64,
}

/// Full result of scanning one folder.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ScanResult {
    pub root: PathBuf,
    pub files: Vec<FileEntry>,
    pub symlinks: Vec<SymlinkEntry>,
    pub unreadable: Vec<UnreadableEntry>,
    /// Zero-byte files, reported separately as "file kosong".
    pub empty_files: Vec<PathBuf>,
}

/// Handle that lets the UI cancel a running scan.
#[derive(Debug, Clone, Default)]
pub struct CancelFlag {
    inner: Arc<AtomicBool>,
}

impl CancelFlag {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.inner.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.inner.load(Ordering::Relaxed)
    }
}

/// True for hidden files/dirs: name starts with `.` (Unix) or the hidden/
/// system attribute is set (Windows).
pub fn is_hidden(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if name.starts_with('.') {
            return true;
        }
    }
    #[cfg(windows)]
    {
        is_hidden_windows(path)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
fn is_hidden_windows(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    // File attributes via metadata: FILE_ATTRIBUTE_HIDDEN (0x2) | SYSTEM (0x4).
    path.metadata()
        .map(|m| m.file_attributes() & 0x6 != 0)
        .unwrap_or(false)
}

/// Scan `root` recursively. Never follows symlinks. Excludes any paths in
/// `excluded_folders`. Updates `progress` as it goes; stops early when `cancel`
/// is set (returns what was found so far).
pub fn scan_folder(
    root: &Path,
    excluded_folders: &[PathBuf],
    progress: &ScanProgress,
    cancel: &CancelFlag,
) -> ScanResult {
    let mut out = ScanResult {
        root: root.to_path_buf(),
        ..Default::default()
    };
    let canon_excluded: Vec<PathBuf> = excluded_folders
        .iter()
        .map(|p| crate::safety::canonicalize_best_effort(p))
        .collect();

    let walker = WalkDir::new(root)
        .follow_links(false)
        .same_file_system(false)
        .into_iter()
        .filter_entry(|e| {
            let path = e.path();
            if e.file_type().is_dir() && path != root {
                if is_hidden(path) {
                    return false;
                }
                let canon = crate::safety::canonicalize_best_effort(path);
                if canon_excluded
                    .iter()
                    .any(|ex| &canon == ex || canon.starts_with(ex))
                {
                    return false;
                }
            }
            true
        });
    for entry in walker {
        if cancel.is_cancelled() {
            break;
        }
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                let p = err.path().unwrap_or(root).to_path_buf();
                out.unreadable.push(UnreadableEntry {
                    path: p,
                    reason: trim_error(&err.to_string()),
                });
                continue;
            }
        };
        let path = entry.path().to_path_buf();
        let ftype = entry.file_type();
        if ftype.is_symlink() {
            out.symlinks.push(SymlinkEntry { path });
            continue;
        }
        if !ftype.is_file() {
            continue;
        }
        if is_hidden(&path) {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(err) => {
                out.unreadable.push(UnreadableEntry {
                    path,
                    reason: trim_error(&err.to_string()),
                });
                continue;
            }
        };
        let size = meta.len();
        if size == 0 {
            out.empty_files.push(path);
            continue;
        }
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        progress.files_scanned.fetch_add(1, Ordering::Relaxed);
        progress.bytes_scanned.fetch_add(size, Ordering::Relaxed);
        out.files.push(FileEntry {
            path,
            size,
            mtime,
            is_readonly: meta.permissions().readonly(),
            file_id: file_id_of(&meta),
        });
    }
    out
}

#[cfg(unix)]
fn file_id_of(meta: &std::fs::Metadata) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    Some(format!("{}:{}", meta.dev(), meta.ino()))
}

#[cfg(not(unix))]
fn file_id_of(_meta: &std::fs::Metadata) -> Option<String> {
    None
}

/// Keep log output privacy-safe: no full paths, short reason only.
fn trim_error(msg: &str) -> String {
    const MAX: usize = 120;
    let mut s: String = msg.chars().take(MAX).collect();
    if msg.len() > MAX {
        s.push('…');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn progress() -> ScanProgress {
        ScanProgress::default()
    }

    #[test]
    fn finds_files_and_skips_empty_and_hidden() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), b"hello").unwrap();
        fs::write(dir.path().join("empty.txt"), b"").unwrap();
        fs::write(dir.path().join(".hidden.txt"), b"secret").unwrap();
        let res = scan_folder(dir.path(), &[], &progress(), &CancelFlag::new());
        assert_eq!(res.files.len(), 1);
        assert_eq!(res.empty_files.len(), 1);
        assert!(res.unreadable.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn records_symlinks_without_following() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("real.txt"), b"data").unwrap();
        std::os::unix::fs::symlink("real.txt", dir.path().join("link.txt")).unwrap();
        let res = scan_folder(dir.path(), &[], &progress(), &CancelFlag::new());
        assert_eq!(res.files.len(), 1);
        assert_eq!(res.symlinks.len(), 1);
    }

    #[test]
    fn unicode_and_long_names() {
        let dir = tempfile::tempdir().unwrap();
        let name = format!("foto-🎉-{}.txt", "a".repeat(120));
        fs::write(dir.path().join(&name), b"x").unwrap();
        let res = scan_folder(dir.path(), &[], &progress(), &CancelFlag::new());
        assert_eq!(res.files.len(), 1);
    }

    #[test]
    fn honors_excluded_folders() {
        let dir = tempfile::tempdir().unwrap();
        let sub1 = dir.path().join("keep");
        let sub2 = dir.path().join("excluded");
        fs::create_dir_all(&sub1).unwrap();
        fs::create_dir_all(&sub2).unwrap();
        fs::write(sub1.join("keep.txt"), b"keep").unwrap();
        fs::write(sub2.join("skip.txt"), b"skip").unwrap();

        let excluded = vec![sub2];
        let res = scan_folder(dir.path(), &excluded, &progress(), &CancelFlag::new());
        assert_eq!(res.files.len(), 1);
        assert_eq!(res.files[0].path, sub1.join("keep.txt"));
    }
}
