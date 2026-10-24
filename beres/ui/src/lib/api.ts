// Typed wrappers for Tauri commands. Mirrors beres-core types serialized
// with serde (snake_case variants, tagged Action unions).

import { invoke } from "@tauri-apps/api/core";

export type Category = "duplikat" | "rapikan" | "installer" | "folder_kosong";

export type Action =
  | { kind: "move"; from: string; to: string }
  | { kind: "quarantine"; path: string; reason: string }
  | { kind: "delete_empty_dir"; path: string }
  | { kind: "rename"; from: string; to: string };

export interface FileEntry {
  path: string;
  size: number;
  mtime: number;
  is_readonly: boolean;
  file_id: string | null;
}

export interface PlanAction {
  id: string;
  action: Action;
  category: Category;
  reason: string;
  bytes_affected: number;
  enabled: boolean;
  group_id: string | null;
}

export interface PlanDupGroup {
  id: string;
  hash: string;
  files: FileEntry[];
  keeper: number;
  reason: string;
  reclaimable_bytes: number;
}

export interface PlanSummary {
  files_found: number;
  duplicate_files: number;
  duplicate_groups: number;
  bytes_reclaimable: number;
  photos_to_move: number;
  installers_archived: number;
  empty_dirs: number;
  total_actions: number;
  enabled_actions: number;
}

export interface Plan {
  root: string;
  actions: PlanAction[];
  dup_groups: PlanDupGroup[];
  summary: PlanSummary;
}

export interface ScanOutcome {
  plan: Plan;
  unreadable_count: number;
  symlink_count: number;
  empty_count: number;
}

export interface ActionOutcome {
  action_id: string;
  status: "done" | "skipped" | "failed";
  note: string;
}

export interface ApplyResult {
  session_id: string;
  outcomes: ActionOutcome[];
  files_moved: number;
  files_quarantined: number;
  cancelled: boolean;
}

export type SessionStatus = "in_progress" | "done" | "undone" | "rolled_back";

export interface SessionInfo {
  id: string;
  started_at: string;
  status: SessionStatus;
  files_moved: number;
  files_quarantined: number;
  label: string;
}

export interface QuarantineRecord {
  id: number;
  original_path: string;
  stored_path: string;
  size: number;
  mtime: number;
  hash: string;
  session_id: string;
  quarantined_at: number;
}

export interface Settings {
  quarantine_dir: string | null;
  retention_days: number;
  excluded_folders: string[];
  auto_update_check: boolean;
  language: string;
  theme: string;
}

function actionTarget(a: Action): string {
  switch (a.kind) {
    case "move":
    case "rename":
      return a.from;
    case "quarantine":
      return a.path;
    case "delete_empty_dir":
      return a.path;
  }
}

export { actionTarget };

export const api = {
  pickFolder: () => invoke<string | null>("pick_folder"),
  validateFolder: (path: string) => invoke<string>("validate_folder", { path }),
  scanFolder: (path: string, mode: string) =>
    invoke<ScanOutcome>("scan_folder_cmd", { path, mode }),
  cancelScan: () => invoke<void>("cancel_scan"),
  cancelApply: () => invoke<void>("cancel_apply"),
  setActionEnabled: (id: string, enabled: boolean) =>
    invoke<PlanSummary>("set_action_enabled", { id, enabled }),
  setAllEnabled: (category: Category | null, enabled: boolean) =>
    invoke<PlanSummary>("set_all_enabled", { category, enabled }),
  setKeeper: (groupId: string, keeper: number) =>
    invoke<Plan>("set_keeper", { groupId, keeper }),
  planNeedsConfirmation: () => invoke<boolean>("plan_needs_confirmation"),
  applyPlan: () => invoke<ApplyResult>("apply_plan_cmd"),
  listSessions: () => invoke<SessionInfo[]>("list_sessions"),
  undoSession: (sessionId: string) =>
    invoke<ActionOutcome[]>("undo_session_cmd", { sessionId }),
  interruptedSessions: () => invoke<SessionInfo[]>("interrupted_sessions"),
  restoreQuarantined: (id: number) => invoke<string>("restore_quarantined", { id }),
  listQuarantined: () => invoke<QuarantineRecord[]>("list_quarantined"),
  purgeQuarantine: (retentionDays: number) =>
    invoke<string[]>("purge_quarantine", { retentionDays }),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
};

export function formatBytes(n: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = n;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  const s = u === 0 ? v.toFixed(0) : v.toFixed(1);
  return `${s.replace(".", ",")} ${units[u]}`;
}
