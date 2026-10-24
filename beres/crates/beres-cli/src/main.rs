//! beres CLI: thin wrapper over beres-core for testing and CI only.
//!
//! Usage:
//!   beres scan <folder> [--mode by-type|by-date] [--json]
//!   beres apply <plan.json>
//!   beres undo <session-id>
//!   beres sessions
//!   beres quarantine list | restore <id> | purge [--retention-days N]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context};

use beres_core::scan::{scan_folder, CancelFlag, ScanProgress};
use beres_core::{apply, cache, dedupe, journal, keeper, plan, quarantine, rules, safety};

fn app_data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("BERES_APP_DATA") {
        return PathBuf::from(dir);
    }
    #[cfg(windows)]
    {
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        base.unwrap_or_else(|| PathBuf::from(".")).join("Beres")
    }
    #[cfg(not(windows))]
    {
        let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
        match base {
            Some(b) => b.join("beres"),
            None => std::env::var_os("HOME")
                .map(|h| PathBuf::from(h).join(".local/share/beres"))
                .unwrap_or_else(|| PathBuf::from(".beres")),
        }
    }
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  beres scan <folder> [--mode by-type|by-date] [--json]");
    eprintln!("  beres apply <plan.json>");
    eprintln!("  beres undo <session-id>");
    eprintln!("  beres sessions");
    eprintln!("  beres quarantine list | restore <id> | purge [--retention-days N]");
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(2);
    }
    match args[1].as_str() {
        "scan" => cmd_scan(&args[2..]),
        "apply" => cmd_apply(&args[2..]),
        "undo" => cmd_undo(&args[2..]),
        "sessions" => cmd_sessions(),
        "quarantine" => cmd_quarantine(&args[2..]),
        _ => {
            print_usage();
            std::process::exit(2);
        }
    }
}

fn cmd_scan(args: &[String]) -> anyhow::Result<()> {
    let mut folder: Option<PathBuf> = None;
    let mut mode = rules::OrganizeMode::ByType;
    let mut json_out = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--mode" => {
                i += 1;
                let m = args.get(i).map(|s| s.as_str()).unwrap_or("");
                mode = match m {
                    "by-date" => rules::OrganizeMode::ByDate,
                    "by-type" => rules::OrganizeMode::ByType,
                    _ => return Err(anyhow!("unknown mode: {m}")),
                };
            }
            "--json" => json_out = true,
            other => {
                if folder.is_none() {
                    folder = Some(PathBuf::from(other));
                } else {
                    return Err(anyhow!("unexpected argument: {other}"));
                }
            }
        }
        i += 1;
    }
    let folder = folder.ok_or_else(|| anyhow!("missing folder"))?;
    let app_data = app_data_dir();
    let root =
        safety::check_scan_root(&folder, &app_data).map_err(|e| anyhow!("folder ditolak: {e}"))?;

    let progress = ScanProgress::default();
    let cancel = CancelFlag::new();
    let scanned = scan_folder(&root, &progress, &cancel);

    let cache_conn = cache::open(&app_data.join("cache.db")).context("membuka cache")?;
    let groups = dedupe::find_duplicates(&scanned.files, Some(&cache_conn));
    let now = chrono_now();
    let plan = plan::build_plan(
        &root,
        &scanned.files,
        &scanned.empty_files,
        &groups,
        mode,
        &rules::RulesConfig::default(),
        &keeper::KeeperConfig::default(),
        now,
    )?;
    // Save the plan next to the app data for `beres apply`.
    let plan_path = app_data.join("last-plan.json");
    std::fs::write(&plan_path, serde_json::to_string_pretty(&plan)?)?;
    if json_out {
        println!("{}", serde_json::to_string(&plan.summary)?);
    } else {
        println!("{}", plan.summary.sentence_id());
        println!("Rencana disimpan di {}", plan_path.display());
        println!(
            "Tidak bisa dibaca: {}, symlink: {}, file kosong: {}",
            scanned.unreadable.len(),
            scanned.symlinks.len(),
            scanned.empty_files.len()
        );
    }
    Ok(())
}

fn cmd_apply(args: &[String]) -> anyhow::Result<()> {
    let plan_path = args.first().ok_or_else(|| anyhow!("missing plan.json"))?;
    let data = std::fs::read_to_string(plan_path)?;
    let plan: plan::Plan = serde_json::from_str(&data)?;
    let app_data = app_data_dir();
    safety::check_scan_root(&plan.root, &app_data).map_err(|e| anyhow!("folder ditolak: {e}"))?;
    let journal_conn = journal::open(&app_data.join("journal.db"))?;
    let q = quarantine::Quarantine::open(&app_data)?;
    // Rebuild fingerprints from current scan? For the CLI we re-scan cheaply.
    let scanned = scan_folder(&plan.root, &ScanProgress::default(), &CancelFlag::new());
    let fps: HashMap<PathBuf, (u64, i64)> = scanned
        .files
        .iter()
        .map(|f| (f.path.clone(), (f.size, f.mtime)))
        .collect();
    let ctx = apply::ApplyContext {
        journal_conn: &journal_conn,
        quarantine: &q,
        app_data_dir: &app_data,
        fingerprints: &fps,
    };
    let res = apply::apply_plan(
        &plan,
        &ctx,
        &apply::ApplyProgress::default(),
        &CancelFlag::new(),
    )?;
    println!(
        "Selesai: {} dipindah, {} dikarantina (sesi {})",
        res.files_moved, res.files_quarantined, res.session_id
    );
    for o in &res.outcomes {
        if !o.note.is_empty() {
            println!("- {}: {}", o.action_id, o.note);
        }
    }
    Ok(())
}

fn cmd_undo(args: &[String]) -> anyhow::Result<()> {
    let session = args.first().ok_or_else(|| anyhow!("missing session-id"))?;
    let app_data = app_data_dir();
    let journal_conn = journal::open(&app_data.join("journal.db"))?;
    let q = quarantine::Quarantine::open(&app_data)?;
    let outcomes = apply::undo_session(&journal_conn, &q, session)?;
    println!(
        "Urungkan sesi {session}: {} aksi dipulihkan",
        outcomes.len()
    );
    Ok(())
}

fn cmd_sessions() -> anyhow::Result<()> {
    let app_data = app_data_dir();
    let journal_conn = journal::open(&app_data.join("journal.db"))?;
    for s in journal::list_sessions(&journal_conn)? {
        println!(
            "{} | {:?} | {} dipindah, {} dikarantina | {}",
            s.id, s.status, s.files_moved, s.files_quarantined, s.label
        );
    }
    Ok(())
}

fn cmd_quarantine(args: &[String]) -> anyhow::Result<()> {
    let app_data = app_data_dir();
    let q = quarantine::Quarantine::open(&app_data)?;
    match args.first().map(|s| s.as_str()) {
        Some("list") => {
            for r in q.list()? {
                println!("{} | {} | {} B", r.id, r.original_path.display(), r.size);
            }
            Ok(())
        }
        Some("restore") => {
            let id: i64 = args.get(1).ok_or_else(|| anyhow!("missing id"))?.parse()?;
            let o = q.restore(id)?;
            println!("Dipulihkan ke {}", o.path().display());
            Ok(())
        }
        Some("purge") => {
            let mut days = 30i64;
            if let Some(pos) = args.iter().position(|a| a == "--retention-days") {
                days = args
                    .get(pos + 1)
                    .ok_or_else(|| anyhow!("missing value"))?
                    .parse()?;
            }
            let purged = q.purge_expired(days, quarantine::now_secs())?;
            println!("{} file dihapus permanen setelah retensi", purged.len());
            Ok(())
        }
        _ => Err(anyhow!(
            "usage: beres quarantine list | restore <id> | purge"
        )),
    }
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[allow(dead_code)]
fn _unused_path(_p: &Path) {}
