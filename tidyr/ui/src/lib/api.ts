// Typed wrappers for Tauri commands. Mirrors tidyr-core types serialized
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

export interface PurgeReport {
  count: number;
  bytes: number;
  purged_paths: string[];
}
export interface Settings {
  // quarantine_dir: omitted by design — quarantine is app-managed in app_data_dir/quarantine
  retention_days: number;
  excluded_folders: string[];
  auto_update_check: boolean;
  language: string;
  theme: string;
  last_update_check_secs: number;
  skipped_version: string | null;
}

export interface QuarantineInfo {
  path: string;
  total_bytes: number;
  count: number;
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

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

let mockPlan: Plan | null = null;
let mockSettings: Settings = {
  retention_days: 30,
  excluded_folders: [],
  auto_update_check: true,
  last_update_check_secs: null,
  skipped_version: null,
  language: "id",
  theme: "system",
};

try {
  const saved = localStorage.getItem("tidyr-mock-settings");
  if (saved) mockSettings = { ...mockSettings, ...JSON.parse(saved) };
} catch {
  /* ignore */
}

function createSamplePlan(root: string): Plan {
  return {
    root,
    actions: [
      {
        id: "dup-0-q1",
        action: {
          kind: "quarantine",
          path: `${root}/laporan (1).pdf`,
          reason: `Duplikat dari ${root}/laporan.pdf`,
        },
        category: "duplikat",
        reason: `File kembar; yang dipertahankan: ${root}/laporan.pdf`,
        bytes_affected: 2048500,
        enabled: true,
        group_id: "dup-0",
      },
      {
        id: "dup-1-q1",
        action: {
          kind: "quarantine",
          path: `${root}/salinan_foto.jpg`,
          reason: `Duplikat dari ${root}/foto_liburan.jpg`,
        },
        category: "duplikat",
        reason: `File kembar; yang dipertahankan: ${root}/foto_liburan.jpg`,
        bytes_affected: 4500000,
        enabled: true,
        group_id: "dup-1",
      },
      {
        id: "move-1",
        action: {
          kind: "move",
          from: `${root}/skripsi_bab1.docx`,
          to: `${root}/Dokumen/skripsi_bab1.docx`,
        },
        category: "rapikan",
        reason: "Dirapikan ke folder Dokumen",
        bytes_affected: 51200,
        enabled: true,
        group_id: null,
      },
      {
        id: "move-2",
        action: {
          kind: "move",
          from: `${root}/Screenshot 2026-10-25.png`,
          to: `${root}/Gambar/Screenshot/Screenshot 2026-10-25.png`,
        },
        category: "rapikan",
        reason: "Dirapikan ke folder Gambar/Screenshot",
        bytes_affected: 1200000,
        enabled: true,
        group_id: null,
      },
      {
        id: "move-3",
        action: {
          kind: "move",
          from: `${root}/old_setup.exe`,
          to: `${root}/Installer lama/old_setup.exe`,
        },
        category: "installer",
        reason: "Installer lama, diarsipkan (tidak dihapus)",
        bytes_affected: 45000000,
        enabled: true,
        group_id: null,
      },
      {
        id: "rmdir-1",
        action: {
          kind: "delete_empty_dir",
          path: `${root}/Folder Kosong Lama`,
        },
        category: "folder_kosong",
        reason: "Folder kosong, dihapus agar rapi",
        bytes_affected: 0,
        enabled: true,
        group_id: null,
      },
    ],
    dup_groups: [
      {
        id: "dup-0",
        hash: "blake3_mock_hash_1",
        files: [
          {
            path: `${root}/laporan.pdf`,
            size: 2048500,
            mtime: 1700000000,
            is_readonly: false,
            file_id: "1",
          },
          {
            path: `${root}/laporan (1).pdf`,
            size: 2048500,
            mtime: 1700005000,
            is_readonly: false,
            file_id: "2",
          },
        ],
        keeper: 0,
        reason: "Dipertahankan karena nama berkas lebih rapi",
        reclaimable_bytes: 2048500,
      },
      {
        id: "dup-1",
        hash: "blake3_mock_hash_2",
        files: [
          {
            path: `${root}/foto_liburan.jpg`,
            size: 4500000,
            mtime: 1690000000,
            is_readonly: false,
            file_id: "3",
          },
          {
            path: `${root}/salinan_foto.jpg`,
            size: 4500000,
            mtime: 1695000000,
            is_readonly: false,
            file_id: "4",
          },
        ],
        keeper: 0,
        reason: "Dipertahankan karena berada di folder utama",
        reclaimable_bytes: 4500000,
      },
    ],
    summary: {
      files_found: 6,
      duplicate_files: 2,
      duplicate_groups: 2,
      bytes_reclaimable: 6548500,
      photos_to_move: 1,
      installers_archived: 1,
      empty_dirs: 1,
      total_actions: 6,
      enabled_actions: 6,
    },
  };
}

export const api = {
  pickFolder: async (): Promise<string | null> => {
    if (isTauri()) return invoke<string | null>("pick_folder");
    return "/home/user/Downloads";
  },
  validateFolder: async (path: string): Promise<string> => {
    if (isTauri()) return invoke<string>("validate_folder", { path });
    return path;
  },
  scanFolder: async (path: string, mode: string): Promise<ScanOutcome> => {
    if (isTauri()) return invoke<ScanOutcome>("scan_folder_cmd", { path, mode });
    await delay(600);
    mockPlan = createSamplePlan(path);
    return {
      plan: mockPlan,
      unreadable_count: 0,
      symlink_count: 0,
      empty_count: 0,
    };
  },
  cancelScan: async (): Promise<void> => {
    if (isTauri()) return invoke<void>("cancel_scan");
  },
  cancelApply: async (): Promise<void> => {
    if (isTauri()) return invoke<void>("cancel_apply");
  },
  setActionEnabled: async (id: string, enabled: boolean): Promise<PlanSummary> => {
    if (isTauri()) return invoke<PlanSummary>("set_action_enabled", { id, enabled });
    if (mockPlan) {
      mockPlan.actions = mockPlan.actions.map((a) =>
        a.id === id ? { ...a, enabled } : a,
      );
      mockPlan.summary.enabled_actions = mockPlan.actions.filter((a) => a.enabled).length;
      return mockPlan.summary;
    }
    return {
      files_found: 0,
      duplicate_files: 0,
      duplicate_groups: 0,
      bytes_reclaimable: 0,
      photos_to_move: 0,
      installers_archived: 0,
      empty_dirs: 0,
      total_actions: 0,
      enabled_actions: 0,
    };
  },
  setAllEnabled: async (
    category: Category | null,
    enabled: boolean,
  ): Promise<PlanSummary> => {
    if (isTauri()) return invoke<PlanSummary>("set_all_enabled", { category, enabled });
    if (mockPlan) {
      mockPlan.actions = mockPlan.actions.map((a) =>
        category === null || a.category === category ? { ...a, enabled } : a,
      );
      mockPlan.summary.enabled_actions = mockPlan.actions.filter((a) => a.enabled).length;
      return mockPlan.summary;
    }
    return {
      files_found: 0,
      duplicate_files: 0,
      duplicate_groups: 0,
      bytes_reclaimable: 0,
      photos_to_move: 0,
      installers_archived: 0,
      empty_dirs: 0,
      total_actions: 0,
      enabled_actions: 0,
    };
  },
  setKeeper: async (groupId: string, keeper: number): Promise<Plan> => {
    if (isTauri()) return invoke<Plan>("set_keeper", { groupId, keeper });
    if (mockPlan) {
      const g = mockPlan.dup_groups.find((grp) => grp.id === groupId);
      if (g) g.keeper = keeper;
      return mockPlan;
    }
    throw new Error("No mock plan");
  },
  planNeedsConfirmation: async (): Promise<boolean> => {
    if (isTauri()) return invoke<boolean>("plan_needs_confirmation");
    return false;
  },
  applyPlan: async (): Promise<ApplyResult> => {
    if (isTauri()) return invoke<ApplyResult>("apply_plan_cmd");
    await delay(800);
    return {
      session_id: "demo-session-web",
      outcomes: [
        {
          action_id: "dup-0-q1",
          status: "done",
          note: "File kembar diamankan ke karantina",
        },
        { action_id: "move-1", status: "done", note: "Dipindahkan ke Dokumen" },
      ],
      files_moved: 3,
      files_quarantined: 2,
      cancelled: false,
    };
  },
  listSessions: async (): Promise<SessionInfo[]> => {
    if (isTauri()) return invoke<SessionInfo[]>("list_sessions");
    return [
      {
        id: "demo-session-web-1",
        started_at: new Date().toISOString(),
        status: "done",
        files_moved: 3,
        files_quarantined: 2,
        label: "Simulasi Sesi Perapian",
      },
    ];
  },
  undoSession: async (sessionId: string): Promise<ActionOutcome[]> => {
    if (isTauri()) return invoke<ActionOutcome[]>("undo_session_cmd", { sessionId });
    return [
      {
        action_id: "demo-1",
        status: "done",
        note: "Dipulihkan kembali ke folder semula",
      },
    ];
  },
  interruptedSessions: async (): Promise<SessionInfo[]> => {
    if (isTauri()) return invoke<SessionInfo[]>("interrupted_sessions");
    return [];
  },
  restoreQuarantined: async (id: number): Promise<string> => {
    if (isTauri()) return invoke<string>("restore_quarantined", { id });
    return "/home/user/Downloads/laporan.pdf";
  },
  listQuarantined: async (): Promise<QuarantineRecord[]> => {
    if (isTauri()) return invoke<QuarantineRecord[]>("list_quarantined");
    return [
      {
        id: 1,
        original_path: "/home/user/Downloads/laporan (1).pdf",
        stored_path: "/home/user/.local/share/id.tidyr.app/quarantine/1.pdf",
        size: 2048500,
        mtime: 1700005000,
        hash: "blake3_mock_hash_1",
        session_id: "demo-session-web-1",
        quarantined_at: Math.floor(Date.now() / 1000) - 86400 * 3,
      },
    ];
  },
  purgeQuarantine: async (retentionDays: number): Promise<PurgeReport> => {
    if (isTauri()) return invoke<PurgeReport>("purge_quarantine", { retentionDays });
    return { count: 0, bytes: 0, purged_paths: [] };
  },
  getSettings: async (): Promise<Settings> => {
    if (isTauri()) return invoke<Settings>("get_settings");
    return mockSettings;
  },
  saveSettings: async (settings: Settings): Promise<void> => {
    if (isTauri()) return invoke<void>("save_settings", { settings });
    mockSettings = settings;
    try {
      localStorage.setItem("tidyr-mock-settings", JSON.stringify(settings));
    } catch {
      /* ignore */
    }
  },
  getQuarantineInfo: async (): Promise<QuarantineInfo> => {
    if (isTauri()) return invoke<QuarantineInfo>("get_quarantine_info");
    return {
      path: "/home/user/.local/share/id.tidyr.app/quarantine",
      count: 1,
      total_bytes: 2048500,
    };
  },
  openQuarantineFolder: async (): Promise<void> => {
    if (isTauri()) return invoke<void>("open_quarantine_folder");
    alert(
      "Folder karantina berada di ~/.local/share/id.tidyr.app/quarantine (Akses native ini aktif pada aplikasi desktop).",
    );
  },
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
