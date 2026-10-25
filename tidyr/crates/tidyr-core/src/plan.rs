//! Plan: typed actions with per-action toggle, summary, and before/after preview.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::classify::format_bytes_id;
use crate::dedupe::DuplicateGroup;
use crate::keeper::{self, KeeperConfig};
use crate::rules::{self, OrganizeMode, RulesConfig};
use crate::scan::FileEntry;
use crate::{Error, Result};

/// Category shown as a tab in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Duplikat,
    Rapikan,
    Installer,
    FolderKosong,
}

impl Category {
    pub fn label_id(self) -> &'static str {
        match self {
            Category::Duplikat => "Duplikat",
            Category::Rapikan => "Rapikan",
            Category::Installer => "Installer lama",
            Category::FolderKosong => "Folder kosong",
        }
    }
}

/// One proposed filesystem change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Move { from: PathBuf, to: PathBuf },
    Quarantine { path: PathBuf, reason: String },
    DeleteEmptyDir { path: PathBuf },
    Rename { from: PathBuf, to: PathBuf },
}

impl Action {
    pub fn target_path(&self) -> &Path {
        match self {
            Action::Move { from, .. } => from,
            Action::Quarantine { path, .. } => path,
            Action::DeleteEmptyDir { path } => path,
            Action::Rename { from, .. } => from,
        }
    }
}

/// A plan action with UI metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanAction {
    pub id: String,
    pub action: Action,
    pub category: Category,
    /// Human-readable Indonesian reason.
    pub reason: String,
    pub bytes_affected: u64,
    pub enabled: bool,
    /// For duplicate groups: which group this belongs to (for group toggles).
    pub group_id: Option<String>,
}

/// Duplicate group info carried in the plan for the group view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanDupGroup {
    pub id: String,
    pub hash: String,
    pub files: Vec<FileEntry>,
    /// Index of the recommended keeper into `files`.
    pub keeper: usize,
    pub reason: String,
    pub reclaimable_bytes: u64,
}

/// Summary numbers for the summary card.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlanSummary {
    pub files_found: usize,
    pub duplicate_files: usize,
    pub duplicate_groups: usize,
    pub bytes_reclaimable: u64,
    pub photos_to_move: usize,
    pub installers_archived: usize,
    pub empty_dirs: usize,
    pub total_actions: usize,
    pub enabled_actions: usize,
}

impl PlanSummary {
    /// Plain-Indonesian one-liner for the summary card.
    pub fn sentence_id(&self) -> String {
        format!(
            "{} file ditemukan. {} duplikat (hemat {}), {} foto dipindah ke folder per bulan, {} installer lama diarsipkan.",
            self.files_found,
            self.duplicate_files,
            format_bytes_id(self.bytes_reclaimable),
            self.photos_to_move,
            self.installers_archived,
        )
    }
}

/// A complete, approvable plan. Dry-run by construction: building it only
/// reads the filesystem (plus the hash cache).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Plan {
    pub root: PathBuf,
    pub actions: Vec<PlanAction>,
    pub dup_groups: Vec<PlanDupGroup>,
    pub summary: PlanSummary,
}

impl Plan {
    pub fn enabled_actions(&self) -> impl Iterator<Item = &PlanAction> {
        self.actions.iter().filter(|a| a.enabled)
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) {
        if let Some(a) = self.actions.iter_mut().find(|a| a.id == id) {
            a.enabled = enabled;
        }
        self.refresh_summary_counts();
    }

    pub fn set_all_enabled(&mut self, category: Option<Category>, enabled: bool) {
        for a in self.actions.iter_mut() {
            if category.map(|c| c == a.category).unwrap_or(true) {
                a.enabled = enabled;
            }
        }
        self.refresh_summary_counts();
    }

    /// Change the keeper for a duplicate group (one click in the UI).
    /// Rebuilds that group's quarantine actions, keeping the invariant.
    pub fn set_keeper(&mut self, group_id: &str, keeper: usize) -> Result<()> {
        let g = self
            .dup_groups
            .iter_mut()
            .find(|g| g.id == group_id)
            .ok_or(Error::EmptyKeeper)?;
        if keeper >= g.files.len() || g.files.is_empty() {
            return Err(Error::EmptyKeeper);
        }
        g.keeper = keeper;
        g.reason = keeper::reason_for(&g.files[keeper]);
        // Drop old quarantine actions for this group, add fresh ones.
        self.actions
            .retain(|a| a.group_id.as_deref() != Some(group_id));
        let mut n = 0usize;
        for (i, f) in g.files.iter().enumerate() {
            if i == keeper {
                continue;
            }
            n += 1;
            self.actions.push(PlanAction {
                id: format!("{group_id}-q{n}"),
                action: Action::Quarantine {
                    path: f.path.clone(),
                    reason: format!("Duplikat dari {}", g.files[keeper].path.display()),
                },
                category: Category::Duplikat,
                reason: format!(
                    "File kembar; yang dipertahankan: {}",
                    g.files[keeper].path.display()
                ),
                bytes_affected: f.size,
                enabled: true,
                group_id: Some(group_id.to_string()),
            });
        }
        self.refresh_summary_counts();
        Ok(())
    }

    fn refresh_summary_counts(&mut self) {
        self.summary.enabled_actions = self.actions.iter().filter(|a| a.enabled).count();
        self.summary.total_actions = self.actions.len();
    }
}

/// Build a plan from a scan: duplicates + organization + empty dirs.
#[allow(clippy::too_many_arguments)]
pub fn build_plan(
    root: &Path,
    files: &[FileEntry],
    empty_files: &[PathBuf],
    groups: &[DuplicateGroup],
    mode: OrganizeMode,
    rules: &RulesConfig,
    keeper_config: &KeeperConfig,
    now_secs: i64,
) -> Result<Plan> {
    let _ = empty_files; // Reported separately; no action proposed for them.
    let mut plan = Plan {
        root: root.to_path_buf(),
        ..Default::default()
    };

    // Duplicate groups -> quarantine actions for non-keepers.
    let mut keepers = Vec::with_capacity(groups.len());
    for (gi, g) in groups.iter().enumerate() {
        let (keeper, reason) = keeper::recommend(g, keeper_config, now_secs)?;
        keepers.push(keeper);
        let gid = format!("dup-{gi}");
        plan.dup_groups.push(PlanDupGroup {
            id: gid.clone(),
            hash: g.hash.clone(),
            files: g.files.clone(),
            keeper,
            reason,
            reclaimable_bytes: g.reclaimable_bytes,
        });
        let mut n = 0usize;
        for (i, f) in g.files.iter().enumerate() {
            if i == keeper {
                continue;
            }
            // Hardlink sets free no space: skip proposing them.
            if g.is_hardlink_set {
                continue;
            }
            n += 1;
            plan.actions.push(PlanAction {
                id: format!("{gid}-q{n}"),
                action: Action::Quarantine {
                    path: f.path.clone(),
                    reason: format!("Duplikat dari {}", g.files[keeper].path.display()),
                },
                category: Category::Duplikat,
                reason: format!(
                    "File kembar; yang dipertahankan: {}",
                    g.files[keeper].path.display()
                ),
                bytes_affected: f.size,
                enabled: true,
                group_id: Some(gid.clone()),
            });
        }
    }
    keeper::validate_keepers(groups, &keepers)?;

    // Organization moves. Skip files already covered by quarantine actions
    // and files already in place.
    let quarantined: BTreeSet<PathBuf> = plan
        .actions
        .iter()
        .filter_map(|a| match &a.action {
            Action::Quarantine { path, .. } => Some(path.clone()),
            _ => None,
        })
        .collect();
    let mut move_n = 0usize;
    for f in files {
        if quarantined.contains(&f.path) {
            continue;
        }
        let Some(dest_dir) = rules::destination_for(f, mode, rules, now_secs) else {
            continue;
        };
        if rules::already_organized(f, root, &dest_dir) {
            continue;
        }
        let file_name = match f.path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let to = root.join(&dest_dir).join(&file_name);
        if to == f.path {
            continue;
        }
        move_n += 1;
        let is_installer = crate::classify::is_installer(&f.path);
        let category = if is_installer && dest_dir == Path::new("Installer lama") {
            Category::Installer
        } else {
            Category::Rapikan
        };
        plan.actions.push(PlanAction {
            id: format!("move-{move_n}"),
            action: Action::Move {
                from: f.path.clone(),
                to,
            },
            category,
            reason: if category == Category::Installer {
                "Installer lama, diarsipkan (tidak dihapus)".to_string()
            } else {
                format!("Dirapikan ke folder {}", dest_dir.display())
            },
            bytes_affected: f.size,
            enabled: true,
            group_id: None,
        });
    }

    // Empty dirs: any dir under root left without files after enabled moves.
    let moved_or_quarantined: BTreeSet<&Path> = plan
        .actions
        .iter()
        .filter_map(|a| match &a.action {
            Action::Move { from, .. } => Some(from.as_path()),
            Action::Quarantine { path, .. } => Some(path.as_path()),
            _ => None,
        })
        .collect();

    let mut dirs_with_remaining_files: BTreeSet<PathBuf> = BTreeSet::new();
    for f in files {
        if !moved_or_quarantined.contains(f.path.as_path()) {
            let mut p = f.path.as_path();
            while let Some(parent) = p.parent() {
                if parent == root || !parent.starts_with(root) {
                    break;
                }
                dirs_with_remaining_files.insert(parent.to_path_buf());
                p = parent;
            }
        }
    }

    let mut dest_dirs: BTreeSet<PathBuf> = BTreeSet::new();
    for a in &plan.actions {
        if let Action::Move { to, .. } = &a.action {
            let mut p = to.as_path();
            while let Some(parent) = p.parent() {
                if parent == root || !parent.starts_with(root) {
                    break;
                }
                dest_dirs.insert(parent.to_path_buf());
                p = parent;
            }
        }
    }

    let mut all_source_dirs: BTreeSet<PathBuf> = BTreeSet::new();
    for f in files {
        let mut p = f.path.as_path();
        while let Some(parent) = p.parent() {
            if parent == root || !parent.starts_with(root) {
                break;
            }
            all_source_dirs.insert(parent.to_path_buf());
            p = parent;
        }
    }

    let mut empty_dirs: Vec<PathBuf> = all_source_dirs
        .into_iter()
        .filter(|d| !dirs_with_remaining_files.contains(d) && !dest_dirs.contains(d))
        .collect();
    empty_dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    let empty_dirs_count = empty_dirs.len();
    for (idx, dir) in empty_dirs.into_iter().enumerate() {
        plan.actions.push(PlanAction {
            id: format!("rmdir-{}", idx + 1),
            action: Action::DeleteEmptyDir { path: dir },
            category: Category::FolderKosong,
            reason: "Folder kosong, dihapus agar rapi".to_string(),
            bytes_affected: 0,
            enabled: true,
            group_id: None,
        });
    }

    plan.summary = PlanSummary {
        files_found: files.len(),
        duplicate_files: plan
            .actions
            .iter()
            .filter(|a| a.category == Category::Duplikat)
            .count(),
        duplicate_groups: groups.len(),
        bytes_reclaimable: groups.iter().map(|g| g.reclaimable_bytes).sum(),
        photos_to_move: plan
            .actions
            .iter()
            .filter(|a| {
                a.category == Category::Rapikan
                    && matches!(&a.action, Action::Move { to, .. } if to.to_string_lossy().contains("Gambar"))
            })
            .count(),
        installers_archived: plan
            .actions
            .iter()
            .filter(|a| a.category == Category::Installer)
            .count(),
        empty_dirs: empty_dirs_count,
        total_actions: plan.actions.len(),
        enabled_actions: plan.actions.len(),
    };
    Ok(plan)
}

/// Before/after preview tree: maps each directory to (before_count, after_count).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PreviewTree {
    pub root: PathBuf,
    pub dirs: BTreeMap<PathBuf, DirPreview>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DirPreview {
    pub files_before: u64,
    pub files_after: u64,
    pub changed: bool,
}

/// Build the preview from the current file list + enabled actions.
pub fn preview_tree(root: &Path, files: &[FileEntry], plan: &Plan) -> PreviewTree {
    let mut dirs: BTreeMap<PathBuf, DirPreview> = BTreeMap::new();
    let mut touch = |dir: PathBuf, before: u64, after: u64| {
        let e = dirs.entry(dir).or_default();
        e.files_before += before;
        e.files_after += after;
    };
    for f in files {
        if let Some(parent) = f.path.parent() {
            touch(parent.to_path_buf(), 1, 0);
        }
    }
    // Simulate enabled actions.
    let mut removed: BTreeSet<&Path> = BTreeSet::new();
    let mut moved_to: Vec<PathBuf> = Vec::new();
    for a in plan.enabled_actions() {
        match &a.action {
            Action::Move { from, to } => {
                removed.insert(from.as_path());
                moved_to.push(to.clone());
            }
            Action::Quarantine { path, .. } => {
                removed.insert(path.as_path());
            }
            Action::DeleteEmptyDir { .. } | Action::Rename { .. } => {}
        }
    }
    for p in &removed {
        if let Some(parent) = p.parent() {
            if let Some(e) = dirs.get_mut(parent) {
                e.files_after = e.files_before.saturating_sub(
                    files
                        .iter()
                        .filter(|f| {
                            f.path.parent() == Some(parent) && removed.contains(f.path.as_path())
                        })
                        .count() as u64,
                );
            }
        }
    }
    for to in &moved_to {
        if let Some(parent) = to.parent() {
            let e = dirs.entry(parent.to_path_buf()).or_default();
            e.files_after += 1;
        }
    }
    // Directories that had no files before but gain some.
    for e in dirs.values_mut() {
        if e.files_after == 0 && e.files_before > 0 && removed.is_empty() {
            e.files_after = e.files_before;
        }
        e.changed = e.files_before != e.files_after;
    }
    // Ensure `to` parents exist in map even if empty before.
    let _ = root;
    PreviewTree {
        root: root.to_path_buf(),
        dirs,
    }
}

/// Large destructive batches need explicit confirmation:
/// > 50 quarantine actions or > 1 GB affected.
pub fn needs_confirmation(plan: &Plan) -> bool {
    let q: Vec<&PlanAction> = plan
        .enabled_actions()
        .filter(|a| matches!(a.action, Action::Quarantine { .. }))
        .collect();
    if q.len() > 50 {
        return true;
    }
    q.iter().map(|a| a.bytes_affected).sum::<u64>() > 1024 * 1024 * 1024
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dedupe::fixture_entry;
    use std::fs;

    fn now() -> i64 {
        1_700_000_000
    }

    #[test]
    fn summary_sentence_is_indonesian() {
        let s = PlanSummary {
            files_found: 1240,
            duplicate_files: 85,
            bytes_reclaimable: 2_100_000_000,
            photos_to_move: 310,
            installers_archived: 40,
            ..Default::default()
        };
        let t = s.sentence_id();
        assert!(t.contains("file ditemukan"));
        assert!(t.contains("diarsipkan"));
    }

    #[test]
    fn build_plan_matches_ground_truth() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("a.pdf"), b"doc-content-12345").unwrap();
        fs::write(root.join("foto.jpg"), b"photo-bytes-1234567").unwrap();
        fs::write(root.join("setup.exe"), b"installer-payload!!").unwrap();
        let files = vec![
            FileEntry {
                path: root.join("a.pdf"),
                size: 15,
                mtime: now(),
                is_readonly: false,
                file_id: None,
            },
            FileEntry {
                path: root.join("foto.jpg"),
                size: 17,
                mtime: now(),
                is_readonly: false,
                file_id: None,
            },
            FileEntry {
                path: root.join("setup.exe"),
                size: 19,
                mtime: 0, // very old -> archived
                is_readonly: false,
                file_id: None,
            },
        ];
        let plan = build_plan(
            root,
            &files,
            &[],
            &[],
            OrganizeMode::ByType,
            &RulesConfig::default(),
            &KeeperConfig::default(),
            now(),
        )
        .unwrap();
        assert_eq!(plan.summary.files_found, 3);
        assert_eq!(plan.summary.installers_archived, 1);
        assert_eq!(plan.actions.len(), 3);
    }

    #[test]
    fn empty_dirs_proposes_rmdir_when_files_moved_away() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let sub = root.join("OldSubfolder");
        fs::create_dir_all(&sub).unwrap();
        let f = sub.join("doc.pdf");
        fs::write(&f, b"hello").unwrap();
        let files = vec![FileEntry {
            path: f,
            size: 5,
            mtime: now(),
            is_readonly: false,
            file_id: None,
        }];
        let plan = build_plan(
            root,
            &files,
            &[],
            &[],
            OrganizeMode::ByType,
            &RulesConfig::default(),
            &KeeperConfig::default(),
            now(),
        )
        .unwrap();
        // 1 move action + 1 rmdir action for OldSubfolder
        assert_eq!(plan.summary.empty_dirs, 1);
        let rmdir_action = plan
            .actions
            .iter()
            .find(|a| a.category == Category::FolderKosong);
        assert!(rmdir_action.is_some());
        assert_eq!(
            rmdir_action.unwrap().action,
            Action::DeleteEmptyDir { path: sub }
        );
    }

    #[test]
    fn set_keeper_rebuilds_group_actions() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("x1.txt"), b"12345678901234567890").unwrap();
        fs::write(dir.path().join("x2.txt"), b"12345678901234567890").unwrap();
        let files = vec![
            fixture_entry(dir.path().join("x1.txt"), 20),
            fixture_entry(dir.path().join("x2.txt"), 20),
        ];
        let groups = crate::dedupe::find_duplicates(&files, None);
        assert_eq!(groups.len(), 1);
        let mut plan = build_plan(
            dir.path(),
            &files,
            &[],
            &groups,
            OrganizeMode::ByType,
            &RulesConfig::default(),
            &KeeperConfig::default(),
            now(),
        )
        .unwrap();
        let gid = plan.dup_groups[0].id.clone();
        let first_keeper = plan.dup_groups[0].keeper;
        plan.set_keeper(&gid, 1 - first_keeper).unwrap();
        assert_eq!(plan.dup_groups[0].keeper, 1 - first_keeper);
        // Still exactly one quarantine action for the 2-file group.
        let qs: Vec<_> = plan
            .actions
            .iter()
            .filter(|a| a.group_id.as_deref() == Some(gid.as_str()))
            .collect();
        assert_eq!(qs.len(), 1);
    }

    #[test]
    fn confirmation_threshold() {
        let mut plan = Plan::default();
        for i in 0..51 {
            plan.actions.push(PlanAction {
                id: format!("q{i}"),
                action: Action::Quarantine {
                    path: PathBuf::from(format!("/x/{i}")),
                    reason: String::new(),
                },
                category: Category::Duplikat,
                reason: String::new(),
                bytes_affected: 1,
                enabled: true,
                group_id: None,
            });
        }
        assert!(needs_confirmation(&plan));
    }
}
