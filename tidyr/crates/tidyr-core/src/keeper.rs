//! Keeper recommendation: exactly one file per duplicate group is kept,
//! with a human-readable Indonesian reason.
//!
//! Scoring signals (documented, configurable, unit-tested):
//! 1. Located in an already well-organized folder beats messy locations.
//! 2. Cleaner filename wins over `copy`, `(1)`, `- Copy`, `salinan`, etc.
//! 3. Older mtime wins (the original is usually the oldest).
//! 4. Shorter path depth as a mild tiebreaker.
//!
//! Hard invariant: at least one file in every group is always kept.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::dedupe::DuplicateGroup;
use crate::scan::FileEntry;
use crate::{Error, Result};

/// Tunable weights for keeper scoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeeperConfig {
    pub organized_bonus: i64,
    pub clean_name_bonus: i64,
    pub messy_name_penalty: i64,
    pub age_bonus_per_day: i64,
    pub depth_penalty_per_level: i64,
}

impl Default for KeeperConfig {
    fn default() -> Self {
        Self {
            organized_bonus: 100,
            clean_name_bonus: 20,
            messy_name_penalty: 50,
            age_bonus_per_day: 1,
            depth_penalty_per_level: 2,
        }
    }
}

/// Folder names considered "messy" (downloads, desktop, temp, ...).
fn messy_segments() -> &'static [&'static str] {
    &[
        "downloads",
        "unduhan",
        "download",
        "desktop",
        "temp",
        "tmp",
        "sampah",
        "trash",
        "whatsapp",
    ]
}

/// Filename fragments that mark a probable copy.
fn messy_name_markers() -> &'static [&'static str] {
    &[
        "(1)", "(2)", "(3)", "(4)", "(5)", "(6)", "(7)", "(8)", "(9)", "copy", "salinan", "- copy",
        "_copy", " copy ", "img_", "vid_",
    ]
}

/// Score a candidate keeper. Higher wins. Pure function of the entry.
pub fn score(entry: &FileEntry, config: &KeeperConfig, now_secs: i64) -> i64 {
    let mut s = 0i64;
    let lower_path = entry.path.to_string_lossy().to_lowercase();
    let messy_here = messy_segments()
        .iter()
        .any(|seg| lower_path.split(['/', '\\']).any(|part| part == *seg));
    if messy_here {
        // No bonus; organized locations get the bonus below.
    } else {
        s += config.organized_bonus;
    }
    if let Some(name) = entry.path.file_name().and_then(|n| n.to_str()) {
        let lower = name.to_lowercase();
        if messy_name_markers().iter().any(|m| lower.contains(m)) {
            s -= config.messy_name_penalty;
        } else {
            s += config.clean_name_bonus;
        }
        // Trailing-number pattern like `foto 1.jpg`, `doc (12).pdf` is messy.
        if has_trailing_number(name) {
            s -= config.messy_name_penalty / 2;
        }
    }
    // Older original wins: days of age add up.
    let age_days = (now_secs - entry.mtime).max(0) / 86_400;
    s += age_days * config.age_bonus_per_day;
    // Shorter path depth wins mildly.
    let depth = entry.path.components().count() as i64;
    s -= depth * config.depth_penalty_per_level;
    s
}

fn has_trailing_number(name: &str) -> bool {
    let stem = match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    };
    let mut t = stem.trim_end();
    let has_closing = if t.ends_with(')') {
        t = &t[..t.len() - 1];
        Some('(')
    } else if t.ends_with(']') {
        t = &t[..t.len() - 1];
        Some('[')
    } else {
        None
    };
    if let Some(idx) = t.rfind(|c: char| !c.is_ascii_digit()) {
        let tail = &t[idx + 1..];
        if tail.is_empty() {
            return false;
        }
        if let Some(opener) = has_closing {
            t[..=idx].ends_with(opener)
        } else {
            t[..=idx].ends_with([' ', '-', '_', '.'])
        }
    } else {
        false
    }
}

/// Recommend the keeper: index into `group.files`, plus Indonesian reason(s).
/// Never returns None for a non-empty group (invariant).
pub fn recommend(
    group: &DuplicateGroup,
    config: &KeeperConfig,
    now_secs: i64,
) -> Result<(usize, String)> {
    if group.files.is_empty() {
        return Err(Error::EmptyKeeper);
    }
    let mut best = 0usize;
    let mut best_score = i64::MIN;
    for (i, f) in group.files.iter().enumerate() {
        let sc = score(f, config, now_secs);
        if sc > best_score {
            best_score = sc;
            best = i;
        }
    }
    Ok((best, reason_for(&group.files[best])))
}

/// Build the Indonesian explanation for why `keeper` was chosen.
pub fn reason_for(keeper: &FileEntry) -> String {
    let mut parts = Vec::new();
    let path = keeper.path.to_string_lossy();
    // Friendly folder hint: parent folder name.
    if let Some(parent) = keeper.path.parent().and_then(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
    }) {
        let lower = parent.to_lowercase();
        let messy = messy_segments().contains(&lower.as_str());
        if messy {
            parts.push("namanya paling rapi di antara file kembar".to_string());
        } else {
            parts.push(format!("berada di folder {parent}"));
        }
    }
    if let Some(name) = keeper.path.file_name().and_then(|n| n.to_str()) {
        let lower = name.to_lowercase();
        if !messy_name_markers().iter().any(|m| lower.contains(m)) {
            parts.push("namanya tidak mengandung '(1)' atau 'copy'".to_string());
        }
    }
    let _ = path;
    format!("Dipertahankan karena {}.", parts.join(" dan "))
}

/// Validate the invariant: every group in `keepers` keeps exactly one file
/// and no group keeps zero. `keepers` maps group index -> keeper index.
pub fn validate_keepers(groups: &[DuplicateGroup], keepers: &[usize]) -> Result<()> {
    if groups.len() != keepers.len() {
        return Err(Error::EmptyKeeper);
    }
    for (g, k) in groups.iter().zip(keepers.iter()) {
        if g.files.is_empty() || *k >= g.files.len() {
            return Err(Error::EmptyKeeper);
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn _path_hint(_p: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dedupe::fixture_entry;
    use std::path::PathBuf;

    fn group(paths: &[&str]) -> DuplicateGroup {
        DuplicateGroup {
            hash: "h".into(),
            files: paths
                .iter()
                .map(|p| fixture_entry(PathBuf::from(p), 10))
                .collect(),
            is_hardlink_set: false,
            reclaimable_bytes: 10,
        }
    }

    #[test]
    fn organized_clean_name_wins() {
        let g = group(&[
            "/home/u/Downloads/laporan (1).pdf",
            "/home/u/Dokumen/Kuliah/laporan.pdf",
        ]);
        let (idx, reason) = recommend(&g, &KeeperConfig::default(), 1_000_000).unwrap();
        assert_eq!(idx, 1);
        assert!(reason.contains("Dipertahankan karena"));
    }

    #[test]
    fn older_original_wins_tiebreak() {
        let mut g = group(&["/d/a.txt", "/d/b.txt"]);
        // Same folder/name quality; make b older.
        g.files[1].mtime = 100;
        g.files[0].mtime = 900_000;
        let (idx, _) = recommend(&g, &KeeperConfig::default(), 1_000_001).unwrap();
        assert_eq!(idx, 1);
    }

    #[test]
    fn empty_group_is_error() {
        let g = DuplicateGroup {
            hash: "h".into(),
            files: vec![],
            is_hardlink_set: false,
            reclaimable_bytes: 0,
        };
        assert!(recommend(&g, &KeeperConfig::default(), 0).is_err());
    }

    #[test]
    fn validate_keepers_enforces_invariant() {
        let groups = vec![group(&["/d/a.txt", "/d/b.txt"])];
        assert!(validate_keepers(&groups, &[0]).is_ok());
        assert!(validate_keepers(&groups, &[2]).is_err());
        assert!(validate_keepers(&groups, &[]).is_err());
    }

    #[test]
    fn reason_example_format() {
        let e = fixture_entry(PathBuf::from("/home/u/Dokumen/Kuliah/tugas.pdf"), 10);
        let r = reason_for(&e);
        assert!(r.starts_with("Dipertahankan karena berada di folder Kuliah"));
    }

    #[test]
    fn has_trailing_number_detects_patterns() {
        assert!(has_trailing_number("foto 1.jpg"));
        assert!(has_trailing_number("doc (12).pdf"));
        assert!(has_trailing_number("scan [3].png"));
        assert!(has_trailing_number("backup-02.zip"));
        assert!(has_trailing_number("file.1.txt"));
        assert!(!has_trailing_number("normal.pdf"));
        assert!(!has_trailing_number("win32.dll"));
        assert!(!has_trailing_number("document.pdf"));
    }
}
