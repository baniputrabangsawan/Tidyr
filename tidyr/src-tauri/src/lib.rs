//! Tidyr Tauri shell: thin glue over tidyr-core. No business logic here.
//!
//! Long-running work (scan/apply) runs on background threads and reports
//! progress via window events (`tidyr://scan-progress`, `tidyr://apply-progress`).
//! Cancellation is cooperative through shared [`CancelFlag`] handles kept in
//! managed state.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tidyr_core::scan::{scan_folder, CancelFlag, ScanProgress};
use tidyr_core::{apply, cache, dedupe, journal, keeper, plan, quarantine, rules, safety};

/// Shared handles for cooperative cancellation of background tasks.
#[derive(Default)]
pub struct TaskState {
    pub scan_cancel: Mutex<Option<CancelFlag>>,
    pub apply_cancel: Mutex<Option<CancelFlag>>,
}

/// Resolve the app data dir via Tauri's path API (mockable in tests).
fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("Tidak bisa membuka folder data aplikasi: {e}"))
}

/// ScanOutcome returned by `scan_folder_cmd` (the plan is cached server-side
/// in a temp file and re-loaded by id for apply; the full plan also goes to
/// the frontend for the Rencana screen).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanOutcome {
    pub plan: plan::Plan,
    pub unreadable_count: usize,
    pub symlink_count: usize,
    pub empty_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressPayload {
    pub files: u64,
    pub bytes: u64,
    pub phase: String,
}

/// Pick a folder with the native dialog. Returns None when cancelled.
#[tauri::command]
async fn pick_folder(app: AppHandle) -> Result<Option<String>, String> {
    let picked = app
        .dialog()
        .file()
        .blocking_pick_folder()
        .map(|fp| fp.into_path().map(|p| p.to_string_lossy().to_string()));
    match picked {
        Some(Ok(s)) => Ok(Some(s)),
        Some(Err(_)) => Err("Folder yang dipilih tidak valid".to_string()),
        None => Ok(None),
    }
}

/// Validate a folder dropped onto the window / typed by the user.
#[tauri::command]
async fn validate_folder(app: AppHandle, path: String) -> Result<String, String> {
    let dir = app_data_dir(&app)?;
    safety::check_scan_root(std::path::Path::new(&path), &dir)
        .map(|p| p.to_string_lossy().to_string())
        // Never leak raw OS paths in the message; keep it friendly.
        .map_err(|_| "Folder ini dilindungi sistem dan tidak bisa dirapikan. Pilih folder seperti Unduhan atau Dokumen.".to_string())
}

/// Run scan + dedupe + plan in a background thread, emitting progress.
#[tauri::command]
async fn scan_folder_cmd(
    app: AppHandle,
    state: State<'_, TaskState>,
    path: String,
    mode: String,
) -> Result<ScanOutcome, String> {
    let dir = app_data_dir(&app)?;
    let root = safety::check_scan_root(std::path::Path::new(&path), &dir)
        .map_err(|_| "Folder ini dilindungi sistem dan tidak bisa dirapikan.".to_string())?;
    let organize_mode = match mode.as_str() {
        "by-date" => rules::OrganizeMode::ByDate,
        _ => rules::OrganizeMode::ByType,
    };

    let cancel = CancelFlag::new();
    *state
        .scan_cancel
        .lock()
        .map_err(|_| "state-race".to_string())? = Some(cancel.clone());
    let progress = ScanProgress::default();
    let app_for_events = app.clone();
    let settings_path = dir.join("settings.json");
    let excluded: Vec<PathBuf> = std::fs::read_to_string(&settings_path)
        .ok()
        .and_then(|d| serde_json::from_str::<Settings>(&d).ok())
        .map(|s| s.excluded_folders.into_iter().map(PathBuf::from).collect())
        .unwrap_or_default();
    let handle = tokio::task::spawn_blocking(move || {
        let scanned = scan_folder(&root, &excluded, &progress, &cancel);
        let _ = app_for_events.emit(
            "tidyr://scan-progress",
            ProgressPayload {
                files: scanned.files.len() as u64,
                bytes: scanned.files.iter().map(|f| f.size).sum(),
                phase: "memindai".to_string(),
            },
        );
        let cache_conn = cache::open(&dir.join("cache.db")).map_err(|e| e.to_string())?;
        let _ = app_for_events.emit(
            "tidyr://scan-progress",
            ProgressPayload {
                files: scanned.files.len() as u64,
                bytes: 0,
                phase: "mencari-duplikat".to_string(),
            },
        );
        let groups = dedupe::find_duplicates(&scanned.files, Some(&cache_conn));
        let now = quarantine::now_secs();
        let built = plan::build_plan(
            &scanned.root,
            &scanned.files,
            &scanned.empty_files,
            &groups,
            organize_mode,
            &rules::RulesConfig::default(),
            &keeper::KeeperConfig::default(),
            now,
        )
        .map_err(|e| e.to_string())?;
        let outcome = ScanOutcome {
            plan: built,
            unreadable_count: scanned.unreadable.len(),
            symlink_count: scanned.symlinks.len(),
            empty_count: scanned.empty_files.len(),
        };
        // Persist the plan + fingerprints for the apply step.
        let fps: HashMap<PathBuf, (u64, i64)> = scanned
            .files
            .iter()
            .map(|f| (f.path.clone(), (f.size, f.mtime)))
            .collect();
        std::fs::write(
            dir.join("last-plan.json"),
            serde_json::to_string(&outcome.plan).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::write(
            dir.join("last-fingerprints.json"),
            serde_json::to_string(&fps).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        Ok::<_, String>(outcome)
    });
    let outcome = handle
        .await
        .map_err(|e| format!("Pemindaian gagal: {e}"))??;
    *state
        .scan_cancel
        .lock()
        .map_err(|_| "state-race".to_string())? = None;
    Ok(outcome)
}

/// Cancel a running scan.
#[tauri::command]
async fn cancel_scan(state: State<'_, TaskState>) -> Result<(), String> {
    if let Some(flag) = state.scan_cancel.lock().map_err(|e| e.to_string())?.take() {
        flag.cancel();
    }
    Ok(())
}

/// Cancel a running apply.
#[tauri::command]
async fn cancel_apply(state: State<'_, TaskState>) -> Result<(), String> {
    if let Some(flag) = state.apply_cancel.lock().map_err(|e| e.to_string())?.take() {
        flag.cancel();
    }
    Ok(())
}

/// Toggle one action in the persisted plan.
#[tauri::command]
async fn set_action_enabled(
    app: AppHandle,
    id: String,
    enabled: bool,
) -> Result<plan::PlanSummary, String> {
    let dir = app_data_dir(&app)?;
    let mut stored: plan::Plan = read_plan(&dir)?;
    stored.set_enabled(&id, enabled);
    write_plan(&dir, &stored)?;
    Ok(stored.summary)
}

/// Toggle all actions of a category (None = all categories).
#[tauri::command]
async fn set_all_enabled(
    app: AppHandle,
    category: Option<plan::Category>,
    enabled: bool,
) -> Result<plan::PlanSummary, String> {
    let dir = app_data_dir(&app)?;
    let mut stored: plan::Plan = read_plan(&dir)?;
    stored.set_all_enabled(category, enabled);
    write_plan(&dir, &stored)?;
    Ok(stored.summary)
}

/// Change the keeper of a duplicate group.
#[tauri::command]
async fn set_keeper(app: AppHandle, group_id: String, keeper: usize) -> Result<plan::Plan, String> {
    let dir = app_data_dir(&app)?;
    let mut stored: plan::Plan = read_plan(&dir)?;
    stored
        .set_keeper(&group_id, keeper)
        .map_err(|_| "Grup duplikat tidak valid.".to_string())?;
    write_plan(&dir, &stored)?;
    Ok(stored)
}

/// Whether the current plan needs the big-batch confirmation dialog.
#[tauri::command]
async fn plan_needs_confirmation(app: AppHandle) -> Result<bool, String> {
    let dir = app_data_dir(&app)?;
    let stored: plan::Plan = read_plan(&dir)?;
    Ok(plan::needs_confirmation(&stored))
}

/// Apply the persisted plan in a background task with progress events.
#[tauri::command]
async fn apply_plan_cmd(
    app: AppHandle,
    state: State<'_, TaskState>,
) -> Result<apply::ApplyResult, String> {
    let dir = app_data_dir(&app)?;
    let stored: plan::Plan = read_plan(&dir)?;
    let fps: HashMap<PathBuf, (u64, i64)> = read_fingerprints(&dir)?;
    let cancel = CancelFlag::new();
    *state
        .apply_cancel
        .lock()
        .map_err(|_| "state-race".to_string())? = Some(cancel.clone());

    let handle = tokio::task::spawn_blocking(move || {
        let journal_conn = journal::open(&dir.join("journal.db")).map_err(|e| e.to_string())?;
        let q = quarantine::Quarantine::open(&dir).map_err(|e| e.to_string())?;
        let ctx = apply::ApplyContext {
            journal_conn: &journal_conn,
            quarantine: &q,
            app_data_dir: &dir,
            fingerprints: &fps,
        };
        let progress = apply::ApplyProgress::default();
        let res =
            apply::apply_plan(&stored, &ctx, &progress, &cancel).map_err(|e| e.to_string())?;
        Ok::<_, String>(res)
    });
    let res = handle
        .await
        .map_err(|e| format!("Penerapan gagal: {e}"))??;
    *state
        .apply_cancel
        .lock()
        .map_err(|_| "state-race".to_string())? = None;
    let _ = app.emit("tidyr://apply-done", &res.session_id);
    Ok(res)
}

/// List past sessions for the Riwayat screen.
#[tauri::command]
async fn list_sessions(app: AppHandle) -> Result<Vec<journal::SessionInfo>, String> {
    let dir = app_data_dir(&app)?;
    let conn = journal::open(&dir.join("journal.db")).map_err(|e| e.to_string())?;
    journal::list_sessions(&conn).map_err(|e| e.to_string())
}

/// Undo one session.
#[tauri::command]
async fn undo_session_cmd(
    app: AppHandle,
    session_id: String,
) -> Result<Vec<apply::ActionOutcome>, String> {
    let dir = app_data_dir(&app)?;
    let conn = journal::open(&dir.join("journal.db")).map_err(|e| e.to_string())?;
    let q = quarantine::Quarantine::open(&dir).map_err(|e| e.to_string())?;
    apply::undo_session(&conn, &q, &session_id)
        .map_err(|_| "Tidak bisa mengurungkan sesi ini.".to_string())
}

/// Sessions interrupted by a crash/kill (offer rollback-or-finish on startup).
#[tauri::command]
async fn interrupted_sessions(app: AppHandle) -> Result<Vec<journal::SessionInfo>, String> {
    let dir = app_data_dir(&app)?;
    let conn = journal::open(&dir.join("journal.db")).map_err(|e| e.to_string())?;
    journal::interrupted_sessions(&conn).map_err(|e| e.to_string())
}

/// Restore one quarantined file by record id.
#[tauri::command]
async fn restore_quarantined(app: AppHandle, id: i64) -> Result<String, String> {
    let dir = app_data_dir(&app)?;
    let q = quarantine::Quarantine::open(&dir).map_err(|e| e.to_string())?;
    q.restore(id)
        .map(|o| o.path().to_string_lossy().to_string())
        .map_err(|_| "File karantina tidak bisa dipulihkan.".to_string())
}

/// List quarantined files.
#[tauri::command]
async fn list_quarantined(app: AppHandle) -> Result<Vec<quarantine::QuarantineRecord>, String> {
    let dir = app_data_dir(&app)?;
    let q = quarantine::Quarantine::open(&dir).map_err(|e| e.to_string())?;
    q.list().map_err(|e| e.to_string())
}

/// Purge expired quarantine entries; returns purged original paths.
#[tauri::command]
async fn purge_quarantine(
    app: AppHandle,
    retention_days: i64,
) -> Result<quarantine::PurgeReport, String> {
    let dir = app_data_dir(&app)?;
    let q = quarantine::Quarantine::open(&dir).map_err(|e| e.to_string())?;
    q.purge_expired(retention_days, quarantine::now_secs())
        .map_err(|e| e.to_string())
}

fn read_plan(dir: &std::path::Path) -> Result<plan::Plan, String> {
    let data = std::fs::read_to_string(dir.join("last-plan.json"))
        .map_err(|_| "Rencana tidak ditemukan. Jalankan Analisis dulu.".to_string())?;
    serde_json::from_str(&data).map_err(|_| "Rencana rusak.".to_string())
}

fn write_plan(dir: &std::path::Path, stored: &plan::Plan) -> Result<(), String> {
    std::fs::write(
        dir.join("last-plan.json"),
        serde_json::to_string(stored).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn read_fingerprints(dir: &std::path::Path) -> Result<HashMap<PathBuf, (u64, i64)>, String> {
    let data = std::fs::read_to_string(dir.join("last-fingerprints.json"))
        .map_err(|_| "Data pindaian tidak ditemukan.".to_string())?;
    serde_json::from_str(&data).map_err(|_| "Data pindaian rusak.".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuarantineInfo {
    pub path: String,
    pub total_bytes: u64,
    pub count: usize,
}

#[tauri::command]
async fn get_quarantine_info(app: AppHandle) -> Result<QuarantineInfo, String> {
    let dir = app_data_dir(&app)?;
    let q = quarantine::Quarantine::open(&dir).map_err(|e| e.to_string())?;
    let records = q.list().map_err(|e| e.to_string())?;
    let total_bytes = records.iter().map(|r| r.size).sum();
    let q_path = dir.join("quarantine").to_string_lossy().to_string();
    Ok(QuarantineInfo {
        path: q_path,
        total_bytes,
        count: records.len(),
    })
}

#[tauri::command]
async fn open_quarantine_folder(app: AppHandle) -> Result<(), String> {
    let dir = app_data_dir(&app)?;
    let q_dir = dir.join("quarantine");
    let _ = std::fs::create_dir_all(&q_dir);

    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(&q_dir).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&q_dir).spawn();
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(&q_dir).spawn();
    }
    Ok(())
}

/// App settings persisted as JSON in the app data dir.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    // quarantine_dir: omitted by design — quarantine is app-managed in app_data_dir/quarantine
    pub retention_days: i64,
    pub excluded_folders: Vec<String>,
    pub auto_update_check: bool,
    pub language: String,
    pub theme: String,
    pub last_update_check_secs: i64,
    pub skipped_version: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            retention_days: 30,
            excluded_folders: Vec::new(),
            auto_update_check: true,
            language: "id".to_string(),
            theme: "system".to_string(),
            last_update_check_secs: 0,
            skipped_version: None,
        }
    }
}

#[tauri::command]
async fn get_settings(app: AppHandle) -> Result<Settings, String> {
    let dir = app_data_dir(&app)?;
    let path = dir.join("settings.json");
    match std::fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).map_err(|_| "Pengaturan rusak.".to_string()),
        Err(_) => Ok(Settings::default()),
    }
}

#[tauri::command]
async fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    let dir = app_data_dir(&app)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join("settings.json"),
        serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(TaskState::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            pick_folder,
            validate_folder,
            scan_folder_cmd,
            cancel_scan,
            cancel_apply,
            set_action_enabled,
            set_all_enabled,
            set_keeper,
            plan_needs_confirmation,
            apply_plan_cmd,
            list_sessions,
            undo_session_cmd,
            interrupted_sessions,
            restore_quarantined,
            list_quarantined,
            purge_quarantine,
            get_quarantine_info,
            open_quarantine_folder,
            get_settings,
            save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
