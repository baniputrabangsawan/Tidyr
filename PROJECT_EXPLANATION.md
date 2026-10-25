# Project Explanation: Tidyr

## 1. Overview & Mission

**Tidyr** (package identifier `id.tidyr.app`, published under repository `Tidyr`, originally conceived as *Beres*) is a lightweight, safe, 100% local desktop application built with **Rust (Tauri v2)** and **Svelte 5**. It helps non-technical users tidy up chaotic directories (such as `Downloads`, `Desktop`, `Pictures`, and WhatsApp media) and safely resolve duplicate files without any risk of data loss.

### The Problem
* **Unchecked Clutter**: Folders like `Downloads` accumulate mixed media, installers, documents, and nested subfolders over months or years.
* **Duplicate Proliferation**: Files are frequently re-downloaded or repeatedly shared via chat apps, creating duplicate variants like `document (1).pdf` or `photo - Copy.jpg`.
* **Fear of Permanent Loss**: Traditional cleanup tools often delete files permanently or route them to system trash bins that are opaque, hard to audit, or difficult to restore accurately.
* **Unwanted Overhead**: Many existing cleaner tools include adware, persistent background daemons, privacy-invasive telemetry, or complex interfaces.

### Core Philosophy
* **Plan-First (Dry-run default)**: Scans never modify the filesystem. Users always preview an actionable plan before any change is made.
* **Zero Permanent Deletion**: Cleaned or duplicate files are placed into an app-managed **Quarantine** with a 30-day retention policy and SQLite registry.
* **100% Reversible**: A write-ahead journal records operations prior to execution, allowing complete one-click rollbacks ("Urungkan").
* **Indonesian UI First**: All user-facing strings are in Indonesian (`ui/src/lib/i18n/id.ts`) to serve everyday users natively, while codebase, comments, and documentation remain in English.
* **Total Privacy**: No analytics, telemetry, user accounts, or external network dependencies (only an optional GitHub Release check for updates).

---

## 2. Architecture & Monorepo Structure

```text
clean-storage/
├── .github/workflows/          # CI and multi-platform automated release pipelines
│   ├── ci.yml                  # Linting (fmt, clippy), unit/acceptance tests, UI check
│   └── release.yml             # Windows NSIS bundle + updater signing on tag push
├── README.md                   # Quickstart and overview
├── PROJECT_EXPLANATION.md      # In-depth architectural and technical explanation
├── .gitignore                  # Git exclusions (Rust target, Node dist, OS artifacts)
└── tidyr/
    ├── Cargo.toml              # Workspace root (resolver="2", size-optimized release profile)
    ├── docs/
    │   └── SAFETY.md           # Formal specification of safety invariants & test mappings
    ├── crates/
    │   ├── tidyr-core/         # Pure Rust domain logic (zero UI/Tauri dependencies)
    │   └── tidyr-cli/          # Headless CLI wrapper for testing, CI, and scripting
    ├── src-tauri/              # Tauri v2 backend glue, commands, and native window hooks
    └── ui/                     # Svelte 5 + Vite 6 + TypeScript desktop UI
```

---

## 3. Technology Stack & Justification

| Layer | Technology | Purpose & Justification |
|---|---|---|
| **Core Logic** | Rust (2021 Edition) | Memory safety, zero-cost abstractions, multi-threaded hashing, and high performance without runtime overhead. |
| **Hashing** | `blake3` | Cryptographically strong and significantly faster than SHA-256 or MD5 for multi-gigabyte scans. |
| **Concurrency** | `rayon` | Automatic work-stealing parallelism for directory traversal and full-file hashing. |
| **Database** | `rusqlite` (bundled) | Embedded zero-dependency SQLite engines for hash caching, quarantine index, and undo logs. |
| **Metadata** | `kamadak-exif` & `filetime` | Extracts EXIF dates from photos/videos and preserves exact filesystem timestamps on cross-volume moves. |
| **Desktop Bridge** | Tauri v2 | Native OS webview host; eliminates the memory footprint of bundling Chromium (unlike Electron). |
| **Frontend** | Svelte 5 + TypeScript + Vite 6 | Minimal runtime overhead, reactive state management, clean component architecture, and plain CSS without framework bloat. |

---

## 4. The Core Engine Pipeline (`tidyr-core`)

All core business logic is housed in `crates/tidyr-core` with no Tauri or GUI dependencies. Operations follow a strict sequential pipeline:

```text
[Target Directory]
       │
       ▼
 1. safety.rs       → Validates path is not an OS, root, or application directory
       │
       ▼
 2. scan.rs         → Recursive traversal; collects metadata (size, mtime, symlinks)
       │
       ▼
 3. classify.rs     → Categorizes files by extension and EXIF timestamps
       │
       ▼
 4. dedupe.rs       → Staged 4-tier exact duplicate detection (BLAKE3 + Rayon)
       │
       ▼
 5. keeper.rs       → Weighted scoring recommends 1 file to keep per group
       │
       ▼
 6. rules.rs        → Calculates organization targets (ByType or ByDate)
       │
       ▼
 7. plan.rs         → Pure function builds preview plan & summary metrics
       │
       ▼
 8. apply.rs        → Pre-flight checks + Write-ahead journaling + execution
```

### 4.1 Safety & Path Protection (`safety.rs`)
Before performing any read or write, candidate paths are normalized and canonicalized to resolve symlinks and prevent `..` directory traversal escapes. It strictly rejects:
* System directories: `C:\Windows`, `Program Files`, `ProgramData`, `/`, `/bin`, `/boot`, `/dev`, `/etc`, `/lib`, `/proc`, `/root`, `/sys`, `/usr`, `/var`.
* User profile roots (`$HOME` or `C:\Users\<Name>`), preventing users from accidentally selecting and reorganizing their whole profile.
* Drive roots (`C:\`, `D:\`, `/`).
* Tidyr's own app data directory (`$APP_DATA/id.tidyr.app`).

### 4.2 Non-Destructive Scanning (`scan.rs`)
* Recursively traverses target folders using `walkdir::WalkDir`.
* Filters out zero-byte files (which cannot reclaim space) and hidden files (starting with `.`).
* Detects symlinks without traversing them, preventing circular symlink loops.
* Uses atomic counters (`ScanProgress`) and supports cooperative, thread-safe cancellation via `CancelFlag` (`AtomicBool`).

### 4.3 Classification & EXIF Metadata (`classify.rs`)
* Groups files into explicit categories:
  * **Dokumen**: `.pdf`, `.docx`, `.xlsx`, `.pptx`, `.txt`, `.csv`, `.md`, etc.
  * **Gambar**: `.jpg`, `.jpeg`, `.png`, `.webp`, `.heic`, `.gif`, `.svg`, etc.
  * **Video**: `.mp4`, `.mkv`, `.avi`, `.mov`, `.wmv`, etc.
  * **Audio**: `.mp3`, `.wav`, `.flac`, `.aac`, `.m4a`, etc.
  * **Arsip**: `.zip`, `.rar`, `.7z`, `.tar`, `.gz`, etc.
  * **Installer**: `.exe`, `.msi`, `.apk`, `.dmg`, `.pkg`, `.deb`, `.rpm`, etc.
* **EXIF Date Parsing**: Extracts capture dates from photo/video EXIF metadata (falling back to file `mtime`), organizing files into `Gambar/YYYY/MM - NamaBulan/` (e.g. `Gambar/2024/08 - Agustus`).
* **Screenshot Detection**: Detects screenshot naming conventions (`Screenshot_*`, `Tangkapan layar_*`, `Screen Shot *`) and routes them directly to `Gambar/Screenshot`.

### 4.4 Staged Duplicate Detection (`dedupe.rs`)
To avoid computing hashes across thousands of files unnecessarily, Tidyr employs a 4-stage filter:
1. **Size Partitioning**: Groups files by exact byte size. Files with unique sizes are skipped immediately with zero read I/O.
2. **Partial Hashing**: Reads the first 64 KB (`PARTIAL_LEN`). For files $> 1\text{ MB}$, it also reads and folds in the last 64 KB.
3. **Full Parallel BLAKE3**: Files matching partial hashes are hashed completely using multi-threaded BLAKE3 via `rayon`.
4. **SQLite Hash Cache (`cache.rs`)**: Computed hashes keyed by `(path, size, mtime)` are cached in SQLite (`cache.db`) so subsequent scans are nearly instantaneous.
5. **Hardlink Awareness**: Hardlinked files sharing the same inode are flagged (`is_hardlink_set`) and excluded from reclaimable space metrics.

### 4.5 Duplicate "Keeper" Selection (`keeper.rs`)
For each duplicate cluster, Tidyr selects exactly one file to **keep** using an automated weighted scoring system:
* **Organized Folder Bonus (+100)**: Prioritizes files in clean, structured directories over messy folders (`downloads`, `unduhan`, `desktop`, `temp`, `sampah`, `whatsapp`).
* **Clean Name Bonus (+20) vs. Copy Penalty (-50)**: Penalizes copy markers like `(1)`, `(2)`, `copy`, `salinan`, `- Copy`.
* **Age Bonus (+1 per day older)**: Original files are typically older than duplicates.
* **Path Depth Penalty (-2 per folder level)**: Prefers shallower paths as a tiebreaker.
* **Human-Readable Rationale**: Produces an Indonesian explanation (e.g., *"Dipertahankan karena berada di folder Foto dan namanya tidak mengandung '(1)' atau 'copy'"*).

### 4.6 Organization Rules (`rules.rs`)
* `OrganizeMode::ByType`: Groups files into category folders (`Dokumen`, `Gambar`, `Video`, etc.).
* `OrganizeMode::ByDate`: Places photos and videos into date folders (`Gambar/YYYY/MM - NamaBulan/`) and other files into their respective type folders.
* **Old Installer Rule**: Installer packages older than 60 days are proposed for relocation to `Installer lama/`.
* **Collision Suffixing (`suffixed_destination`)**: If a destination path already exists, automatically appends numeric suffixes (`document (2).pdf`), preventing overwrites.

### 4.7 Plan Synthesis (`plan.rs`)
Transforms scan results into an immutable `Plan` object:
* Contains typed action items: `Move`, `Quarantine`, `DeleteEmptyDir`, `Rename`.
* UI toggles allow users to enable/disable specific actions, toggle entire categories, or manually change the recommended keeper.
* Operations exceeding 50 quarantines or 1 GB of data require explicit confirmation.

---

## 5. Safety, Quarantine & Undo System

### Safety Guarantees (`docs/SAFETY.md`)
1. **No Permanent Deletions Outside Retention Purge**: Files marked for cleanup are moved to an isolated quarantine store. The only permanent deletion is the purge of expired quarantine files after retention (default: 30 days).
2. **Keeper Invariant**: Under no circumstance can a duplicate group retain zero files.
3. **Overwrite Immunity**: Target paths are checked before moving; cross-volume copies enforce `create_new(true)`.
4. **Write-Ahead Logging (WAL)**: All steps are recorded in `journal.db` before execution. If the process is terminated mid-apply, startup detects the interrupted session and offers immediate rollback.
5. **Changed-Since-Scan Check**: File sizes and `mtime` timestamps are verified immediately before moving. If a file changed after scanning, it is skipped and logged.

### Quarantine Storage (`quarantine.rs`)
Quarantined files are moved into the app data directory:
```text
$APP_DATA/quarantine/
├── quarantine.db      # SQLite records: id, original_path, stored_path, size, mtime, hash, session_id, quarantined_at
└── files/             # Safely encapsulated file storage
```
* **Restoration**: Individual or batch restoration restores files to their original paths (auto-suffixing if the original path is now occupied).

---

## 6. Desktop UI & Tauri Integration

### User Interface Screens (`ui/src/lib/components/`)
* **`Home.svelte` (Beranda)**: Folder picker (or drag-and-drop), organization mode toggle (`ByType` vs `ByDate`), recent folder list.
* **`Scan.svelte` (Memindai)**: Animated background progress bar with live metrics (scanned count, unreadable files, symlinks) and cancel button.
* **`PlanView.svelte` (Rencana)**: High-level metric summary card, category navigation tabs (`Duplikat`, `Rapikan`, `Installer lama`, `Folder kosong`), keeper selector dropdowns, and before/after previews.
* **`Applying.svelte` (Menerapkan)**: Live progress of journaled execution with real-time counters.
* **`Done.svelte` (Selesai)**: Execution summary with a prominent **1-click "Urungkan"** button.
* **`History.svelte` (Riwayat)**: Historical session log with rollback options and quarantine browser with individual file restoration.
* **`SettingsView.svelte` (Pengaturan)**: Theme options (System / Dark / Light), quarantine retention duration slider, and manual update checks.

---

## 7. Local File Storage Layout

All persistent metadata is stored locally within the OS application data directory (`~/.local/share/id.tidyr.app/` on Linux, `%APPDATA%\id.tidyr.app\` on Windows):

```text
id.tidyr.app/
├── settings.json         # User settings (theme, retention_days, organize_mode)
├── cache.db              # SQLite hash cache (size, mtime, blake3)
├── journal.db            # SQLite write-ahead undo journal
└── quarantine/
    ├── quarantine.db     # SQLite registry of quarantined files
    └── files/            # Quarantined files
```

---

## 8. Verification & Development Guide

### 8.1 Automated Test Suites
```sh
cd tidyr

# Run all core tests (52 unit, acceptance, and property tests)
cargo test -p tidyr-core

# Run CLI tests and linting
cargo test -p tidyr-cli
cargo clippy -p tidyr-core -p tidyr-cli --all-targets -- -D warnings
cargo fmt -p tidyr-core -p tidyr-cli -- --check
```

### 8.2 Headless CLI Smoke Test
You can test the entire scan $\to$ plan $\to$ apply $\to$ undo lifecycle without launching the GUI:
```sh
# 1. Prepare sample files
mkdir -p /tmp/tidyr-demo
echo "Document content" > /tmp/tidyr-demo/sample.pdf
echo "Document content" > /tmp/tidyr-demo/"sample (1).pdf"
echo "My Notes"         > /tmp/tidyr-demo/notes.txt

# 2. Build the CLI
cd tidyr
cargo build -p tidyr-cli

# 3. Scan (Dry-Run)
export TIDYR_APP_DATA=/tmp/tidyr-data
./target/debug/tidyr scan /tmp/tidyr-demo

# 4. Apply the plan
./target/debug/tidyr apply /tmp/tidyr-data/last-plan.json

# 5. Inspect quarantine and undo
./target/debug/tidyr quarantine list
./target/debug/tidyr sessions
./target/debug/tidyr undo <SESSION_ID>

```

### 8.3 Frontend Type-Checking & Build
```sh
cd tidyr/ui
npm ci
npm run check
npm run build
```

### 8.4 Running Desktop App in Development
```sh
cd tidyr

# Linux prerequisites:
# sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf

# Run desktop app
npx @tauri-apps/cli dev
```

---

## 9. Non-Goals & Boundaries

By design, Tidyr intentionally excludes:
* **Perceptual / Fuzzy Hashing**: Only exact byte duplicates are matched in the MVP to eliminate false positives.
* **Cloud Sync / Remote Storage**: Everything runs locally with zero external network connectivity.
* **Persistent Background Watchers**: Operates strictly on-demand to avoid continuous memory and CPU drain.
* **Direct Trash API**: Avoids inconsistent OS Recycle Bin implementations across different operating systems and file systems by managing its own SQLite-backed quarantine.
