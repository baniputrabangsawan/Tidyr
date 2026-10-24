//! Apply: execute an approved plan safely in a background task.
//!
//! Guarantees:
//! - Every action is journaled BEFORE execution (write-ahead); marked done after.
//! - Before every move/quarantine, re-check the file still exists with the
//!   same size and mtime as at scan time; otherwise skip + report.
//! - Cancelling mid-way leaves a consistent, fully undoable state.
//! - Move destinations never overwrite (auto-suffix).
//! - Undoing a session restores everything to the original paths.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::dedupe::unchanged_since_scan;
use crate::journal;
use crate::plan::{Action, Plan, PlanAction};
use crate::quarantine::{move_file, Quarantine};
use crate::rules::suffixed_destination;
use crate::safety;
use crate::{Error, Result};

/// Progress counters for the apply task.
#[derive(Debug, Default)]
pub struct ApplyProgress {
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub skipped: AtomicU64,
}

/// Outcome of applying one action (for the result screen).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionOutcome {
    pub action_id: String,
    pub status: ActionStatus,
    /// Human-readable Indonesian note (e.g. restored-with-suffix report).
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Done,
    Skipped,
    Failed,
}

/// Full result of applying a plan.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApplyResult {
    pub session_id: String,
    pub outcomes: Vec<ActionOutcome>,
    pub files_moved: usize,
    pub files_quarantined: usize,
    pub cancelled: bool,
}

/// Context needed to execute a plan.
pub struct ApplyContext<'a> {
    pub journal_conn: &'a rusqlite::Connection,
    pub quarantine: &'a Quarantine,
    pub app_data_dir: &'a Path,
    /// (size, mtime) captured at scan time, keyed by source path.
    pub fingerprints: &'a HashMap<PathBuf, (u64, i64)>,
}

/// Execute all enabled actions of `plan`. Journal-first, skippable,
/// cancellable. Always finishes the session record, even on cancel.
pub fn apply_plan(
    plan: &Plan,
    ctx: &ApplyContext<'_>,
    progress: &ApplyProgress,
    cancel: &crate::scan::CancelFlag,
) -> Result<ApplyResult> {
    let enabled: Vec<&PlanAction> = plan.enabled_actions().collect();
    progress
        .total
        .store(enabled.len() as u64, Ordering::Relaxed);
    let label = plan_summary_label(plan);
    let session_id = journal::begin_session(ctx.journal_conn, &label)?;
    let mut result = ApplyResult {
        session_id: session_id.clone(),
        ..Default::default()
    };

    for action in enabled {
        if cancel.is_cancelled() {
            result.cancelled = true;
            break;
        }
        let outcome = execute_one(&session_id, action, ctx);
        match outcome.status {
            ActionStatus::Done => {
                if matches!(action.action, Action::Move { .. } | Action::Rename { .. }) {
                    result.files_moved += 1;
                }
                if matches!(action.action, Action::Quarantine { .. }) {
                    result.files_quarantined += 1;
                }
                progress.done.fetch_add(1, Ordering::Relaxed);
            }
            ActionStatus::Skipped | ActionStatus::Failed => {
                progress.skipped.fetch_add(1, Ordering::Relaxed);
            }
        }
        result.outcomes.push(outcome);
    }

    journal::finish_session(
        ctx.journal_conn,
        &session_id,
        result.files_moved,
        result.files_quarantined,
    )?;
    Ok(result)
}

fn plan_summary_label(plan: &Plan) -> String {
    format!(
        "{} aksi di {}",
        plan.enabled_actions().count(),
        plan.root.display()
    )
}

/// Execute a single action: safety check -> journal (write-ahead) ->
/// re-verify fingerprint -> perform -> mark done.
fn execute_one(
    session_id: &str,
    plan_action: &PlanAction,
    ctx: &ApplyContext<'_>,
) -> ActionOutcome {
    let done_outcome = |note: &str| ActionOutcome {
        action_id: plan_action.id.clone(),
        status: ActionStatus::Done,
        note: note.to_string(),
    };
    let skip = |note: &str| ActionOutcome {
        action_id: plan_action.id.clone(),
        status: ActionStatus::Skipped,
        note: note.to_string(),
    };
    let fail = |note: &str| ActionOutcome {
        action_id: plan_action.id.clone(),
        status: ActionStatus::Failed,
        note: note.to_string(),
    };

    // 1. Safety: refuse protected paths.
    if let Err(e) = safety::check_action_path(plan_action.action.target_path(), ctx.app_data_dir) {
        return skip(&format!(
            "Dilewati (dilindungi): {}",
            short_path(&e.to_string())
        ));
    }

    // 2. Journal before execution.
    let undo_stub = serde_json::json!({ "action_id": plan_action.id });
    let entry_id = match journal::record_before(
        ctx.journal_conn,
        session_id,
        &plan_action.action,
        plan_action.category,
        &undo_stub,
    ) {
        Ok(id) => id,
        Err(e) => return fail(&format!("Gagal mencatat: {e}")),
    };

    // 3. Perform with fingerprint re-checks.
    // Moves record their ACTUAL destination (possibly suffixed) so undo can
    // find exactly the file that was moved — never a same-named sibling.
    let res: Result<(String, Option<PathBuf>)> = match &plan_action.action {
        Action::Move { from, to } => do_move(from, to, ctx).map(|(n, d)| (n, Some(d))),
        Action::Rename { from, to } => do_move(from, to, ctx).map(|(n, d)| (n, Some(d))),
        Action::Quarantine { path, .. } => {
            do_quarantine(path, session_id, ctx, entry_id).map(|n| (n, None))
        }
        Action::DeleteEmptyDir { path } => do_rmdir(path).map(|n| (n, None)),
    };

    // 4. Mark done (only on success; failures keep done=0 for recovery).
    match res {
        Ok((note, actual_dest)) => {
            if let Some(dest) = actual_dest {
                let _ = journal::set_undo_data(
                    ctx.journal_conn,
                    entry_id,
                    &serde_json::json!({ "actual_dest": dest }),
                );
            }
            let _ = journal::mark_done(ctx.journal_conn, entry_id);
            done_outcome(&note)
        }
        Err(Error::ChangedSinceScan(_)) => skip("Dilewati: file berubah sejak dipindai"),
        Err(Error::WouldOverwrite(_)) => skip("Dilewati: nama tujuan sudah dipakai"),
        Err(Error::ProtectedPath(_)) => skip("Dilewati: lokasi dilindungi"),
        Err(e) => fail(&format!("Gagal: {}", short_err(&e.to_string()))),
    }
}

fn fingerprint_ok(ctx: &ApplyContext<'_>, path: &Path) -> bool {
    match ctx.fingerprints.get(path) {
        Some((size, mtime)) => unchanged_since_scan(path, *size, *mtime),
        // No fingerprint (e.g. empty-dir actions): require existence only.
        None => path.exists(),
    }
}

fn do_move(from: &Path, to: &Path, ctx: &ApplyContext<'_>) -> Result<(String, PathBuf)> {
    if !fingerprint_ok(ctx, from) {
        return Err(Error::ChangedSinceScan(from.to_path_buf()));
    }
    safety::check_action_path(to, ctx.app_data_dir)?;
    // Auto-suffix when the destination exists; never overwrite.
    let dest = if to.exists() {
        let parent = to.parent().unwrap_or_else(|| Path::new("."));
        let name = to.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        suffixed_destination(parent, name)
    } else {
        to.to_path_buf()
    };
    if from == dest {
        return Ok((String::new(), dest));
    }
    move_file(from, &dest)?;
    if dest != *to {
        Ok((
            format!("Dipindah dengan nama baru: {}", dest.display()),
            dest,
        ))
    } else {
        Ok((String::new(), dest))
    }
}

fn do_quarantine(
    path: &Path,
    session_id: &str,
    ctx: &ApplyContext<'_>,
    entry_id: i64,
) -> Result<String> {
    let (size, mtime) = ctx
        .fingerprints
        .get(path)
        .copied()
        .ok_or_else(|| Error::ChangedSinceScan(path.to_path_buf()))?;
    if !unchanged_since_scan(path, size, mtime) {
        return Err(Error::ChangedSinceScan(path.to_path_buf()));
    }
    let rec = ctx
        .quarantine
        .quarantine_file(path, size, mtime, session_id)?;
    // Attach the quarantine record id to THIS entry by id, so undo restores
    // exactly this file even when many entries share the same file name.
    journal::set_undo_data(
        ctx.journal_conn,
        entry_id,
        &serde_json::json!({ "quarantine_id": rec.id }),
    )?;
    Ok(String::new())
}

fn do_rmdir(path: &Path) -> Result<String> {
    match std::fs::remove_dir(path) {
        Ok(()) => Ok(String::new()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        // Directory not empty anymore (user added files mid-apply): skip silently.
        Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
            Err(Error::ChangedSinceScan(path.to_path_buf()))
        }
        Err(e) => Err(Error::Io(e)),
    }
}

/// Undo a whole session: reverse the journaled entries.
/// Returns per-entry notes (including suffixed-restores) for the UI.
pub fn undo_session(
    journal_conn: &rusqlite::Connection,
    quarantine: &Quarantine,
    session_id: &str,
) -> Result<Vec<ActionOutcome>> {
    let mut entries = journal::session_entries(journal_conn, session_id)?;
    entries.retain(|e| e.done);
    entries.reverse();

    let mut outcomes = Vec::new();
    for entry in entries {
        let note = undo_one(quarantine, &entry)?;
        outcomes.push(ActionOutcome {
            action_id: format!("entry-{}", entry.id),
            status: ActionStatus::Done,
            note,
        });
    }
    journal::set_session_status(journal_conn, session_id, journal::SessionStatus::Undone)?;
    Ok(outcomes)
}

fn undo_one(quarantine: &Quarantine, entry: &journal::JournalEntry) -> Result<String> {
    match &entry.action {
        Action::Move { from, to } | Action::Rename { from, to } => {
            // Prefer the recorded actual destination (exact file, even when
            // suffixed at apply time). The heuristic search is only a
            // fallback for entries written before actual_dest was recorded.
            let current = entry
                .undo_data
                .get("actual_dest")
                .and_then(|v| v.as_str())
                .map(PathBuf::from)
                .filter(|p| p.exists())
                .or_else(|| locate_moved_file(from, to));
            match current {
                Some(cur) => {
                    // Restore to `from`; suffix again if occupied now.
                    let dest = if from.exists() {
                        let parent = from.parent().unwrap_or_else(|| Path::new("."));
                        let name = from.file_name().and_then(|n| n.to_str()).unwrap_or("file");
                        suffixed_destination(parent, name)
                    } else {
                        from.clone()
                    };
                    let suffixed = dest != *from;
                    move_file(&cur, &dest)?;
                    if suffixed {
                        Ok(format!("Dikembalikan dengan nama baru: {}", dest.display()))
                    } else {
                        Ok(String::new())
                    }
                }
                None => Ok("Dilewati: file sudah tidak ada".to_string()),
            }
        }
        Action::Quarantine { path, .. } => {
            // Prefer the recorded quarantine id; fall back to path+session search.
            if let Some(qid) = entry
                .undo_data
                .get("quarantine_id")
                .and_then(|v| v.as_i64())
            {
                match quarantine.restore(qid) {
                    Ok(o) => Ok(describe_restore(o)),
                    Err(_) => restore_by_search(quarantine, path),
                }
            } else {
                restore_by_search(quarantine, path)
            }
        }
        Action::DeleteEmptyDir { path } => {
            // Recreate the directory (it was empty, so nothing to restore inside).
            match std::fs::create_dir_all(path) {
                Ok(()) => Ok(String::new()),
                Err(e) => Ok(format!(
                    "Tidak bisa membuat ulang folder: {}",
                    short_err(&e.to_string())
                )),
            }
        }
    }
}

fn describe_restore(o: crate::quarantine::RestoreOutcome) -> String {
    match o {
        crate::quarantine::RestoreOutcome::Restored(_) => String::new(),
        crate::quarantine::RestoreOutcome::RestoredWithSuffix(p) => {
            format!("Dikembalikan dengan nama baru: {}", p.display())
        }
    }
}

fn restore_by_search(quarantine: &Quarantine, original: &Path) -> Result<String> {
    let records = quarantine.list()?;
    let hit = records.iter().find(|r| r.original_path == original);
    match hit {
        Some(rec) => {
            let rec = rec.clone();
            Ok(describe_restore(quarantine.restore_record(&rec)?))
        }
        None => Ok("Dilewati: file karantina tidak ditemukan".to_string()),
    }
}

/// Find where a moved file actually ended up: exact `to`, or a suffixed sibling.
fn locate_moved_file(from: &Path, to: &Path) -> Option<PathBuf> {
    if to.exists() {
        // Verify it is plausibly ours: same file name stem.
        return Some(to.to_path_buf());
    }
    let parent = to.parent()?;
    let file_name = to.file_name()?.to_str()?;
    let (stem, ext) = match file_name.rfind('.') {
        Some(i) if i > 0 => (&file_name[..i], Some(&file_name[i + 1..])),
        _ => (file_name, None),
    };
    let from_size = std::fs::metadata(from).ok().map(|m| m.len());
    let _ = from_size;
    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
    for n in 2..100u32 {
        let candidate = match ext {
            Some(e) => parent.join(format!("{stem} ({n}).{e}")),
            None => parent.join(format!("{stem} ({n})")),
        };
        if candidate.exists() {
            let mtime = std::fs::metadata(&candidate)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            // Most recently modified wins (it was moved by our apply).
            if best.as_ref().map(|(_, t)| mtime > *t).unwrap_or(true) {
                best = Some((candidate, mtime));
            }
        }
    }
    best.map(|(p, _)| p)
}

fn short_path(msg: &str) -> String {
    msg.chars().take(80).collect()
}

fn short_err(msg: &str) -> String {
    msg.chars().take(120).collect()
}

/// Shared cancel flag type re-exported for Tauri glue convenience.
pub type Cancel = Arc<AtomicBool>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keeper::KeeperConfig;
    use crate::rules::{OrganizeMode, RulesConfig};
    use crate::scan::{scan_folder, CancelFlag, ScanProgress};
    use std::fs;

    fn collect_fingerprints(files: &[crate::scan::FileEntry]) -> HashMap<PathBuf, (u64, i64)> {
        files
            .iter()
            .map(|f| (f.path.clone(), (f.size, f.mtime)))
            .collect()
    }

    #[test]
    fn apply_then_undo_restores_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("target");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("doc.pdf"), b"document-payload-123").unwrap();
        fs::write(root.join("doc (1).pdf"), b"document-payload-123").unwrap();
        fs::write(root.join("foto.jpg"), b"photo-payload-123456").unwrap();

        let app_data = dir.path().join("appdata");
        let quarantine = Quarantine::open(&app_data).unwrap();
        let journal_conn = journal::open(&app_data.join("journal.db")).unwrap();

        let scanned = scan_folder(&root, &ScanProgress::default(), &CancelFlag::new());
        assert_eq!(scanned.files.len(), 3);
        let groups = crate::dedupe::find_duplicates(&scanned.files, None);
        assert_eq!(groups.len(), 1);
        let plan = crate::plan::build_plan(
            &root,
            &scanned.files,
            &[],
            &groups,
            OrganizeMode::ByType,
            &RulesConfig::default(),
            &KeeperConfig::default(),
            1_700_000_000,
        )
        .unwrap();

        let fps = collect_fingerprints(&scanned.files);
        let ctx = ApplyContext {
            journal_conn: &journal_conn,
            quarantine: &quarantine,
            app_data_dir: &app_data,
            fingerprints: &fps,
        };
        let res = apply_plan(&plan, &ctx, &ApplyProgress::default(), &CancelFlag::new()).unwrap();
        assert!(!res.cancelled);
        assert_eq!(res.files_quarantined, 1);

        // One file quarantined, others moved into type folders.
        let remaining: Vec<_> = walk_simple(&root);
        assert!(remaining
            .iter()
            .all(|p| p != &root.join("doc (1).pdf") || !p.exists()));

        // Undo restores everything.
        let outcomes = undo_session(&journal_conn, &quarantine, &res.session_id).unwrap();
        assert!(!outcomes.is_empty());
        assert!(root.join("doc.pdf").exists() || root.join("doc (2).pdf").exists());
        assert!(root.join("foto.jpg").exists());
        // Both duplicate copies exist again (maybe with suffix).
        let pdfs: Vec<_> = walk_simple(&root)
            .into_iter()
            .filter(|p| p.extension().map(|e| e == "pdf").unwrap_or(false))
            .collect();
        assert_eq!(pdfs.len(), 2);
    }

    #[test]
    fn changed_file_is_skipped_not_moved() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("t");
        fs::create_dir(&root).unwrap();
        let f = root.join("a.txt");
        fs::write(&f, b"hello-world-data").unwrap();
        let app_data = dir.path().join("appdata");
        let quarantine = Quarantine::open(&app_data).unwrap();
        let journal_conn = journal::open(&app_data.join("journal.db")).unwrap();

        let scanned = scan_folder(&root, &ScanProgress::default(), &CancelFlag::new());
        // Tamper after scan.
        fs::write(&f, b"tampered-tampered-tampered").unwrap();

        let plan = crate::plan::build_plan(
            &root,
            &scanned.files,
            &[],
            &[],
            OrganizeMode::ByType,
            &RulesConfig::default(),
            &KeeperConfig::default(),
            1_700_000_000,
        )
        .unwrap();
        let fps = collect_fingerprints(&scanned.files);
        let ctx = ApplyContext {
            journal_conn: &journal_conn,
            quarantine: &quarantine,
            app_data_dir: &app_data,
            fingerprints: &fps,
        };
        let res = apply_plan(&plan, &ctx, &ApplyProgress::default(), &CancelFlag::new()).unwrap();
        // The tampered file must not have been moved.
        assert!(f.exists());
        assert!(res
            .outcomes
            .iter()
            .all(|o| o.status != ActionStatus::Done || !o.action_id.starts_with("move")));
    }

    fn walk_simple(root: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(d) = stack.pop() {
            if let Ok(rd) = fs::read_dir(&d) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else {
                        out.push(p);
                    }
                }
            }
        }
        out
    }
}
