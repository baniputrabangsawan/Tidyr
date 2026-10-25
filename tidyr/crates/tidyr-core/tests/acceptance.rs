//! Acceptance tests for tidyr-core.
//!
//! - Fixture: 5,000 mixed files with 500 duplicates; the plan summary must
//!   match the fixture ground truth.
//! - Property: apply(plan) then undo(session) restores the exact original
//!   tree (compared by path + hash sets).
//! - Crash recovery: a session recorded but never finished shows up as
//!   interrupted and can be rolled back.
//!
//! NEVER run against real user folders: everything lives in tempdirs.

use proptest::prelude::*;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use tidyr_core::apply::{self, ApplyContext};
use tidyr_core::journal;
use tidyr_core::keeper::KeeperConfig;
use tidyr_core::plan;
use tidyr_core::quarantine::{self, Quarantine};
use tidyr_core::rules::{OrganizeMode, RulesConfig};
use tidyr_core::scan::{scan_folder, CancelFlag, FileEntry, ScanProgress};

/// Deterministic pseudo-random bytes (xorshift64) so fixtures are stable.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn fill(&mut self, buf: &mut [u8]) {
        let mut i = 0;
        while i < buf.len() {
            let v = self.next().to_le_bytes();
            let n = (buf.len() - i).min(8);
            buf[i..i + n].copy_from_slice(&v[..n]);
            i += n;
        }
    }
}

/// Build the acceptance fixture:
///
/// - 4,500 unique files spread over nested dirs (mixed extensions, Unicode
///   and long names included)
/// - 500 extra files that are exact duplicates of 500 of the unique ones
///
/// Returns (unique_count, duplicate_count).
fn build_fixture(root: &Path) -> (usize, usize) {
    let mut rng = Rng(0x1234_5678_9abc_def0);
    let exts = ["pdf", "jpg", "mp4", "mp3", "zip", "txt", "png", "docx"];
    let mut contents: Vec<Vec<u8>> = Vec::new();
    // 4,500 unique files.
    for i in 0..4500usize {
        let dir = root.join(format!("dir{:03}/sub{}", i % 90, i % 7));
        fs::create_dir_all(&dir).unwrap();
        let mut name = format!("file-{i:05}.{}", exts[i % exts.len()]);
        if i % 500 == 0 {
            name = format!("foto-liburan-🎉-{i}-{}.jpg", "a".repeat(60));
        }
        let mut buf = vec![0u8; 64 + (i % 900)];
        rng.fill(&mut buf);
        // Make each unique: fold the index into the first bytes.
        buf[0..8].copy_from_slice(&(i as u64).to_le_bytes());
        fs::write(dir.join(&name), &buf).unwrap();
        if i % 9 == 0 {
            contents.push(buf);
        }
    }
    // 500 duplicates: copies of earlier files with messy copy-like names.
    for (k, content) in contents.iter().take(500).enumerate() {
        let dir = root.join(format!("Downloads/dup{k:03}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("file ({}).bin", k % 9 + 1)), content).unwrap();
    }
    (4500, 500)
}

fn snapshot_tree(root: &Path) -> BTreeMap<PathBuf, String> {
    let mut map = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let rel = p.strip_prefix(root).unwrap().to_path_buf();
                map.insert(rel, quarantine::blake3_file(&p).unwrap());
            }
        }
    }
    map
}

fn fingerprints_of(files: &[FileEntry]) -> HashMap<PathBuf, (u64, i64)> {
    files
        .iter()
        .map(|f| (f.path.clone(), (f.size, f.mtime)))
        .collect()
}

#[test]
fn acceptance_plan_matches_ground_truth() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("target");
    fs::create_dir(&root).unwrap();
    let (unique, dups) = build_fixture(&root);

    let scanned = scan_folder(&root, &[], &ScanProgress::default(), &CancelFlag::new());
    assert_eq!(
        scanned.files.len(),
        unique + dups,
        "all fixture files must be scanned"
    );

    let groups = tidyr_core::dedupe::find_duplicates(&scanned.files, None);
    // 500 duplicate pairs -> 500 groups of 2.
    assert_eq!(groups.len(), 500, "expected 500 duplicate groups");
    assert!(groups.iter().all(|g| g.files.len() == 2));

    let built = plan::build_plan(
        &root,
        &scanned.files,
        &scanned.empty_files,
        &groups,
        OrganizeMode::ByType,
        &RulesConfig::default(),
        &KeeperConfig::default(),
        1_700_000_000,
    )
    .unwrap();
    assert_eq!(built.summary.files_found, unique + dups);
    assert_eq!(built.summary.duplicate_groups, 500);
    assert_eq!(built.summary.duplicate_files, 500);
    assert!(built.summary.bytes_reclaimable > 0);
    // Invariant: every group keeps exactly one file.
    for g in &built.dup_groups {
        assert!(g.keeper < g.files.len());
    }
}

#[test]
fn acceptance_apply_then_undo_restores_tree() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("target");
    fs::create_dir(&root).unwrap();
    build_fixture(&root);
    let before = snapshot_tree(&root);

    let app_data = dir.path().join("appdata");
    let q = Quarantine::open(&app_data).unwrap();
    let journal_conn = journal::open(&app_data.join("journal.db")).unwrap();

    let scanned = scan_folder(&root, &[], &ScanProgress::default(), &CancelFlag::new());
    let groups = tidyr_core::dedupe::find_duplicates(&scanned.files, None);
    let built = plan::build_plan(
        &root,
        &scanned.files,
        &scanned.empty_files,
        &groups,
        OrganizeMode::ByType,
        &RulesConfig::default(),
        &KeeperConfig::default(),
        1_700_000_000,
    )
    .unwrap();
    let fps = fingerprints_of(&scanned.files);
    let ctx = ApplyContext {
        journal_conn: &journal_conn,
        quarantine: &q,
        app_data_dir: &app_data,
        fingerprints: &fps,
    };
    let res = apply::apply_plan(
        &built,
        &ctx,
        &apply::ApplyProgress::default(),
        &CancelFlag::new(),
    )
    .unwrap();
    assert!(!res.cancelled);
    assert_eq!(res.files_quarantined, 500);

    // Simulate crash recovery bookkeeping: session is done, none interrupted.
    assert!(journal::interrupted_sessions(&journal_conn)
        .unwrap()
        .is_empty());

    apply::undo_session(&journal_conn, &q, &res.session_id).unwrap();
    let after = snapshot_tree(&root);
    // Compare by hash multisets: same content set restored (names may carry
    // a suffix when the original path got occupied mid-test, which cannot
    // happen here, so require exact equality).
    assert_eq!(before, after, "tree must be restored exactly");
}

#[test]
fn crash_recovery_offers_rollback() {
    let dir = tempfile::tempdir().unwrap();
    let app_data = dir.path().join("appdata");
    let journal_conn = journal::open(&app_data.join("journal.db")).unwrap();
    // Simulate a kill mid-apply: session begun, action journaled, never done.
    let sid = journal::begin_session(&journal_conn, "test").unwrap();
    let action = plan::Action::Move {
        from: PathBuf::from("/a"),
        to: PathBuf::from("/b"),
    };
    journal::record_before(
        &journal_conn,
        &sid,
        &action,
        plan::Category::Rapikan,
        &serde_json::json!({}),
    )
    .unwrap();
    let interrupted = journal::interrupted_sessions(&journal_conn).unwrap();
    assert_eq!(interrupted.len(), 1);
    journal::set_session_status(&journal_conn, &sid, journal::SessionStatus::RolledBack).unwrap();
    assert!(journal::interrupted_sessions(&journal_conn)
        .unwrap()
        .is_empty());
}

#[test]
fn crash_recovery_mid_apply_and_rollback() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("target");
    fs::create_dir(&root).unwrap();
    let sub = root.join("downloads");
    fs::create_dir_all(&sub).unwrap();

    fs::write(sub.join("a.txt"), b"duplicate content").unwrap();
    fs::write(sub.join("a (1).txt"), b"duplicate content").unwrap();
    fs::write(sub.join("b.txt"), b"unique doc").unwrap();
    fs::write(sub.join("c.txt"), b"another doc").unwrap();

    let before = snapshot_tree(&root);

    let app_data = dir.path().join("appdata");
    let q = Quarantine::open(&app_data).unwrap();
    let journal_conn = journal::open(&app_data.join("journal.db")).unwrap();

    let scanned = scan_folder(&root, &[], &ScanProgress::default(), &CancelFlag::new());
    let groups = tidyr_core::dedupe::find_duplicates(&scanned.files, None);
    let built = plan::build_plan(
        &root,
        &scanned.files,
        &scanned.empty_files,
        &groups,
        OrganizeMode::ByType,
        &RulesConfig::default(),
        &KeeperConfig::default(),
        1_700_000_000,
    )
    .unwrap();

    let fps = fingerprints_of(&scanned.files);
    let _ctx = ApplyContext {
        journal_conn: &journal_conn,
        quarantine: &q,
        app_data_dir: &app_data,
        fingerprints: &fps,
    };

    // Simulate crash after applying partial actions:
    let sid = journal::begin_session(&journal_conn, root.to_str().unwrap()).unwrap();
    let first = &built.actions[0];
    let undo_data = match &first.action {
        plan::Action::Move { from, to } => {
            quarantine::move_file(from, to).unwrap();
            serde_json::json!({ "actual_dest": to })
        }
        plan::Action::Quarantine { path, .. } => {
            let meta = fs::metadata(path).unwrap();
            let rec = q.quarantine_file(path, meta.len(), 123, &sid).unwrap();
            serde_json::json!({ "quarantine_id": rec.id })
        }
        _ => serde_json::json!({}),
    };
    let entry_id = journal::record_before(
        &journal_conn,
        &sid,
        &first.action,
        first.category,
        &undo_data,
    )
    .unwrap();
    journal::mark_done(&journal_conn, entry_id).unwrap();

    // Process crashed! Session status is still InProgress.
    let interrupted = journal::interrupted_sessions(&journal_conn).unwrap();
    assert_eq!(interrupted.len(), 1);
    assert_eq!(interrupted[0].id, sid);

    // Recovery path invoked by UI
    let outcomes = apply::undo_session(&journal_conn, &q, &sid).unwrap();
    assert_eq!(outcomes.len(), 1);

    // Verify status is RolledBack and tree is restored
    assert!(journal::interrupted_sessions(&journal_conn)
        .unwrap()
        .is_empty());
    let after = snapshot_tree(&root);
    assert_eq!(
        before, after,
        "tree must be restored after crash recovery rollback"
    );
}

#[test]
fn acceptance_cancel_mid_apply_leaves_consistent_and_undo_restores() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("target");
    fs::create_dir(&root).unwrap();
    let sub = root.join("files");
    fs::create_dir_all(&sub).unwrap();

    for i in 0..12 {
        fs::write(
            sub.join(format!("doc_{i}.txt")),
            format!("unique content {i}").as_bytes(),
        )
        .unwrap();
    }
    for i in 0..4 {
        fs::write(
            sub.join(format!("dup_{i}.bin")),
            format!("dup content {i}").as_bytes(),
        )
        .unwrap();
        fs::write(
            sub.join(format!("dup_{i} (1).bin")),
            format!("dup content {i}").as_bytes(),
        )
        .unwrap();
    }

    let before = snapshot_tree(&root);

    let app_data = dir.path().join("appdata");
    let q = Quarantine::open(&app_data).unwrap();
    let journal_conn = journal::open(&app_data.join("journal.db")).unwrap();

    let scanned = scan_folder(&root, &[], &ScanProgress::default(), &CancelFlag::new());
    let groups = tidyr_core::dedupe::find_duplicates(&scanned.files, None);
    let built = plan::build_plan(
        &root,
        &scanned.files,
        &scanned.empty_files,
        &groups,
        OrganizeMode::ByType,
        &RulesConfig::default(),
        &KeeperConfig::default(),
        1_700_000_000,
    )
    .unwrap();

    assert!(built.actions.len() >= 10);

    let fps = fingerprints_of(&scanned.files);
    let ctx = ApplyContext {
        journal_conn: &journal_conn,
        quarantine: &q,
        app_data_dir: &app_data,
        fingerprints: &fps,
    };

    let cancel = CancelFlag::new();
    let cancel_clone = cancel.clone();
    let progress = Arc::new(apply::ApplyProgress::default());
    let prog_clone = progress.clone();

    let cancel_thread = thread::spawn(move || {
        while prog_clone.done.load(std::sync::atomic::Ordering::Relaxed) < 2 {
            thread::yield_now();
        }
        cancel_clone.cancel();
    });

    let res = apply::apply_plan(&built, &ctx, &progress, &cancel).unwrap();
    cancel_thread.join().unwrap();

    assert!(res.cancelled);
    let sessions = journal::list_sessions(&journal_conn).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].status, journal::SessionStatus::Done);

    let _ = apply::undo_session(&journal_conn, &q, &res.session_id).unwrap();

    let after = snapshot_tree(&root);
    assert_eq!(
        before, after,
        "tree must match exactly after undoing a cancelled session"
    );
}

// Property test: random small trees, apply then undo == original (path+hash sets).
proptest! {
    #![proptest_config(ProptestConfig::with_cases(
        std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(20)
    ))]
    #[test]
    fn prop_apply_undo_is_identity(seed in 0u64..1_000_000u64) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("t");
        fs::create_dir(&root).unwrap();
        let mut rng = Rng(seed ^ 0x9e37_79b9_7f4a_7c15);
        // Random tree: 5..40 files, some duplicated.
        let n_files = 5 + (rng.next() % 35) as usize;
        let mut pool: Vec<Vec<u8>> = Vec::new();
        for i in 0..n_files {
            let reuse = !pool.is_empty() && rng.next().is_multiple_of(3);
            let content = if reuse {
                pool[(rng.next() as usize) % pool.len()].clone()
            } else {
                let mut buf = vec![0u8; 1 + (rng.next() % 2000) as usize];
                rng.fill(&mut buf);
                let tag = (seed ^ i as u64).to_le_bytes();
                let n = 8.min(buf.len());
                buf[0..n].copy_from_slice(&tag[..n]);
                pool.push(buf.clone());
                buf
            };
            let sub = root.join(format!("d{}", rng.next() % 4));
            fs::create_dir_all(&sub).unwrap();
            fs::write(sub.join(format!("f{i}.dat")), &content).unwrap();
        }
        let before = snapshot_tree(&root);

        let app_data = dir.path().join("appdata");
        let q = Quarantine::open(&app_data).unwrap();
        let journal_conn = journal::open(&app_data.join("journal.db")).unwrap();
        let scanned = scan_folder(&root, &[], &ScanProgress::default(), &CancelFlag::new());
        let groups = tidyr_core::dedupe::find_duplicates(&scanned.files, None);
        let built = plan::build_plan(
            &root, &scanned.files, &scanned.empty_files, &groups,
            OrganizeMode::ByType, &RulesConfig::default(), &KeeperConfig::default(), 1_700_000_000,
        ).unwrap();
        let fps = fingerprints_of(&scanned.files);
        let ctx = ApplyContext {
            journal_conn: &journal_conn, quarantine: &q,
            app_data_dir: &app_data, fingerprints: &fps,
        };
        let res = apply::apply_plan(&built, &ctx, &apply::ApplyProgress::default(), &CancelFlag::new()).unwrap();
        prop_assert!(!res.cancelled);
        // No group ever ends with zero files: quarantine count == groups' non-keeper count.
        let expected_q: usize = groups.iter().map(|g| if g.is_hardlink_set { 0 } else { g.files.len() - 1 }).sum();
        prop_assert_eq!(res.files_quarantined, expected_q);
        apply::undo_session(&journal_conn, &q, &res.session_id).unwrap();
        let after = snapshot_tree(&root);
        prop_assert_eq!(before, after);
    }
}

#[ignore]
#[test]
fn bench_100k_files() {
    use std::time::Instant;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("bench_100k");
    fs::create_dir_all(&root).unwrap();

    let exts = ["pdf", "jpg", "txt", "png", "docx"];
    let num_files = 100_000usize;
    let num_dups = 5_000usize;

    println!("\n=== Generating 100,000-file fixture ===");
    let start_gen = Instant::now();
    for d in 0..100 {
        fs::create_dir_all(root.join(format!("d{d:02}"))).unwrap();
    }
    fs::create_dir_all(root.join("dup_dir")).unwrap();

    for i in 0..(num_files - num_dups) {
        let p = root.join(format!("d{:02}/f_{i:06}.{}", i % 100, exts[i % exts.len()]));
        let buf = format!("unique content {i}").into_bytes();
        fs::write(p, buf).unwrap();
    }
    for i in 0..num_dups {
        let p = root.join(format!("dup_dir/copy_{i:05}.pdf"));
        let buf = format!("unique content {i}").into_bytes();
        fs::write(p, buf).unwrap();
    }
    println!(
        "Fixture generation completed in {:.2?}",
        start_gen.elapsed()
    );

    let app_data = dir.path().join("appdata");
    let cache_path = app_data.join("cache.db");

    // 1. Cold Scan (no cache)
    println!("=== 1. Cold Scan ===");
    let start_cold = Instant::now();
    let scanned_cold = scan_folder(&root, &[], &ScanProgress::default(), &CancelFlag::new());
    let cache_conn = tidyr_core::cache::open(&cache_path).unwrap();
    let groups = tidyr_core::dedupe::find_duplicates(&scanned_cold.files, Some(&cache_conn));
    let cold_dur = start_cold.elapsed();
    println!("Cold scan + dedupe: {:.2?} (target: <= 30s)", cold_dur);
    assert_eq!(scanned_cold.files.len(), 100_000);
    assert!(!groups.is_empty());

    // 2. Warm Scan (populated cache)
    println!("=== 2. Warm Scan ===");
    let start_warm = Instant::now();
    let scanned_warm = scan_folder(&root, &[], &ScanProgress::default(), &CancelFlag::new());
    let groups_warm = tidyr_core::dedupe::find_duplicates(&scanned_warm.files, Some(&cache_conn));
    let warm_dur = start_warm.elapsed();
    println!("Warm scan + dedupe: {:.2?} (target: <= 10s)", warm_dur);
    assert_eq!(scanned_warm.files.len(), 100_000);
    assert_eq!(groups_warm.len(), groups.len());

    // 3. Plan
    println!("=== 3. Plan ===");
    let start_plan = Instant::now();
    let built = plan::build_plan(
        &root,
        &scanned_warm.files,
        &scanned_warm.empty_files,
        &groups_warm,
        OrganizeMode::ByType,
        &RulesConfig::default(),
        &KeeperConfig::default(),
        1_700_000_000,
    )
    .unwrap();
    let plan_dur = start_plan.elapsed();
    println!("Plan build: {:.2?}", plan_dur);
    assert!(built.actions.len() >= 5_000);

    // 4. Memory check
    #[cfg(target_os = "linux")]
    if let Ok(status) = fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if line.starts_with("VmHWM:") || line.starts_with("VmRSS:") {
                println!("{line}");
            }
        }
    }
}
