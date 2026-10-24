//! Organization rules/templates: how proposed MOVE destinations are computed.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::classify::{classify_by_extension, date_folder_name, is_screenshot, photo_date};
use crate::scan::FileEntry;

/// Organization mode, selectable per run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OrganizeMode {
    /// Dokumen, Gambar, Video, ... (+ Gambar/Screenshot special case).
    #[default]
    ByType,
    /// Photos/videos to Gambar/<YYYY>/<MM - NamaBulan>/, the rest by type.
    ByDate,
}

/// Tunable thresholds for organization rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RulesConfig {
    /// Installers older than this many days are proposed to "Installer lama".
    pub old_installer_days: i64,
    /// Seconds of tolerance, reserved for future use.
    pub date_fallback_to_mtime: bool,
}

impl Default for RulesConfig {
    fn default() -> Self {
        Self {
            old_installer_days: 60,
            date_fallback_to_mtime: true,
        }
    }
}

/// Compute the destination directory (relative to the scan root) for a file
/// under the given mode. Returns None when the file is already organized
/// (no move proposed).
pub fn destination_for(
    entry: &FileEntry,
    mode: OrganizeMode,
    config: &RulesConfig,
    now_secs: i64,
) -> Option<PathBuf> {
    let file_name = entry.path.file_name()?.to_str()?.to_string();
    let category = classify_by_extension(&entry.path);

    // Screenshots always go to Gambar/Screenshot.
    if matches!(category, crate::classify::FileCategory::Gambar) && is_screenshot(&file_name, None)
    {
        return Some(PathBuf::from("Gambar/Screenshot"));
    }

    let dest = match mode {
        OrganizeMode::ByType => PathBuf::from(category.folder_name()),
        OrganizeMode::ByDate => match category {
            crate::classify::FileCategory::Gambar | crate::classify::FileCategory::Video => {
                let (y, m, _) = photo_date(&entry.path, entry.mtime);
                PathBuf::from("Gambar").join(date_folder_name(y, m))
            }
            _ => PathBuf::from(category.folder_name()),
        },
    };
    // Old installers go to "Installer lama" instead (move, never delete).
    if crate::classify::is_installer(&entry.path)
        && now_secs - entry.mtime > config.old_installer_days * 86_400
    {
        Some(PathBuf::from("Installer lama"))
    } else {
        Some(dest)
    }
}

/// True when the file already lives directly inside `dest` relative to root.
pub fn already_organized(entry: &FileEntry, root: &Path, dest: &Path) -> bool {
    if let Ok(rel) = entry.path.strip_prefix(root) {
        if let Some(parent) = rel.parent() {
            return parent == dest;
        }
    }
    false
}

/// Suggest a non-colliding file name inside `dir`: `name (2).ext`, `name (3).ext`, ...
/// Never overwrites; purely lexical (checks the filesystem, no writes).
pub fn suffixed_destination(dir: &Path, file_name: &str) -> PathBuf {
    let candidate = dir.join(file_name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = split_name(file_name);
    let mut n = 2u32;
    loop {
        let next = match ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        };
        let p = dir.join(next);
        if !p.exists() {
            return p;
        }
        n += 1;
    }
}

fn split_name(file_name: &str) -> (&str, Option<&str>) {
    match file_name.rfind('.') {
        Some(i) if i > 0 => (&file_name[..i], Some(&file_name[i + 1..])),
        _ => (file_name, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, mtime: i64) -> FileEntry {
        FileEntry {
            path: PathBuf::from(path),
            size: 10,
            mtime,
            is_readonly: false,
            file_id: None,
        }
    }

    #[test]
    fn by_type_routes_documents() {
        let e = entry("/root/laporan.pdf", 0);
        let d = destination_for(&e, OrganizeMode::ByType, &RulesConfig::default(), 0);
        assert_eq!(d, Some(PathBuf::from("Dokumen")));
    }

    #[test]
    fn screenshots_go_to_screenshot_folder() {
        let e = entry("/root/Screenshot 2024-01-01.png", 0);
        let d = destination_for(&e, OrganizeMode::ByType, &RulesConfig::default(), 0);
        assert_eq!(d, Some(PathBuf::from("Gambar/Screenshot")));
    }

    #[test]
    fn old_installers_archived() {
        let e = entry("/root/setup.exe", 0);
        let d = destination_for(
            &e,
            OrganizeMode::ByType,
            &RulesConfig::default(),
            61 * 86_400,
        );
        assert_eq!(d, Some(PathBuf::from("Installer lama")));
    }

    #[test]
    fn young_installers_stay() {
        let e = entry("/root/setup.exe", 59 * 86_400);
        let d = destination_for(
            &e,
            OrganizeMode::ByType,
            &RulesConfig::default(),
            59 * 86_400,
        );
        assert_eq!(d, Some(PathBuf::from("Installer")));
    }

    #[test]
    fn conflict_suffixing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("foto.jpg"), b"x").unwrap();
        let p = suffixed_destination(dir.path(), "foto.jpg");
        assert_eq!(p.file_name().unwrap(), "foto (2).jpg");
        std::fs::write(&p, b"x").unwrap();
        let p3 = suffixed_destination(dir.path(), "foto.jpg");
        assert_eq!(p3.file_name().unwrap(), "foto (3).jpg");
    }
}
