# Tidyr — rapikan folder, atasi file kembar, tanpa takut kehilangan file

Tidyr is a safe, friendly, 100% local desktop app (Windows first; macOS/Linux
compile best-effort) that helps non-technical users clean messy folders like
Downloads, Desktop, Pictures, and WhatsApp media. All user-facing text is in
**Indonesian** (see `tidyr/ui/src/lib/i18n/id.ts`; mirror it to add English later).
Code, comments, and docs are in English.

> Screenshots: _placeholder — add `docs/screenshot-*.png` after first release._

## How it works (plan-first)

1. **Beranda** — pick a folder (picker or drag-and-drop), choose organize mode.
2. **Memindai** — background scan with progress + cancel. Nothing is modified.
3. **Rencana** — human-readable summary card, per-category tabs with
   checkboxes, duplicate groups with keeper badges + reasons, before/after
   preview. Large batches (>50 quarantines or >1 GB) need explicit confirmation.
4. **Menerapkan** — journaled apply with progress + cancel.
5. **Selesai / Riwayat** — one-click **Urungkan** per session; individual
   quarantined files can be restored too.

Design references (Mobbin, per project workflow): cleaner summary-card +
category-row patterns as in WhatsApp/Telegram storage screens, and the
scan → duplicates-review flow shape.
 WhatsApp storage usage · Telegram storage usage · "Duplicates"/"Scan for duplicates" flows (Mesh, iOS).

## Safety guarantees

- **Nothing is permanently deleted**, except quarantine purge after retention
  (default 30 days, configurable, with a startup notice).
- **Every duplicate group keeps at least one file** — enforced in core.
- **No overwrites**: auto-suffix (`name (2).ext`), `create_new` copies.
- **Write-ahead journal**: every action recorded before execution; kill the app
  mid-apply and restart offers rollback; cancelling leaves undoable state.
- **Changed-since-scan detection**: files are re-checked (size + mtime) before
  every move/quarantine; changed files are skipped + reported.
- **Protected paths** refused with a clear message (`C:\Windows`,
  `C:\Program Files`, drive roots, `/`, `/etc`, home root, app data dir…).
- **Private**: no account, no ads, no telemetry. The only network call is the
  optional update check (see below). Logs contain counts, not file paths.

See [`tidyr/docs/SAFETY.md`](tidyr/docs/SAFETY.md) for the invariant → test mapping.

## Architecture

```text
tidyr/
├── crates/tidyr-core/   # ALL logic, zero UI/Tauri deps (safety→scan→classify→
│                        # dedupe/keeper→plan→journal/quarantine→apply)
├── crates/tidyr-cli/    # thin CLI for testing/CI only (`tidyr scan|apply|undo|…`)
├── src-tauri/           # thin Tauri commands (glue only, no business logic)
├── ui/                  # Svelte + Vite + TypeScript, plain CSS, Indonesian i18n
└── .github/workflows/   # ci.yml (test/clippy/fmt/build) + release.yml (tag v*)
```

## Build & run

Prerequisites: Rust stable, Node 22+, and (Linux only) webkitGTK system libs —
see `ci.yml`. Windows/macOS need no extra system deps.

```sh
# Desktop app (dev)
cd tidyr && cargo tauri dev

# Desktop app (release installer; Windows NSIS per-user, no admin needed)
cd tidyr && cargo tauri build

# Core + CLI only (no system webview deps needed)
cd tidyr
cargo test -p tidyr-core -p tidyr-cli
cargo clippy -p tidyr-core -p tidyr-cli --all-targets -- -D warnings
cargo fmt -p tidyr-core -p tidyr-cli -- --check

# UI only
cd tidyr/ui && npm ci && npm run check && npm run build

# CLI smoke test (uses TIDYR_APP_DATA to avoid touching real data)
export TIDYR_APP_DATA=/tmp/tidyr-smoke
cd tidyr && cargo run -p tidyr-cli -- scan ~/Downloads --json
```

Note: `src-tauri` is a workspace member but needs OS webviews to compile, so
local gates use `-p tidyr-core -p tidyr-cli`; CI checks `-p tidyr` on all OSes.

## Dependencies (justified)

`walkdir` (traversal), `rayon` (parallel hashing), `blake3` (hashing),
`kamadak-exif` (EXIF dating), `chrono` (dates), `rusqlite/bundled` (cache +
journal), `serde/serde_json`, `thiserror`, `anyhow`, `tracing`,
`tracing-subscriber`, `filetime` (preserve timestamps on cross-volume moves),
`tempfile` + `proptest` (dev only). Frontend: `svelte`, `vite`, `typescript`,
`svelte-check`, `@tauri-apps/api`, `plugin-dialog`, `plugin-updater` — no UI
frameworks, no CSS frameworks.

## Updater key setup (maintainer)

1. Generate a keypair: `cargo tauri signer generate -w ~/.tauri/tidyr.key`
   (or `tauri signer generate`). The `.pubkey` output goes into
   `tidyr/src-tauri/tauri.conf.json` → `plugins.updater.pubkey`.
2. Store the private key in GitHub Secrets as `TAURI_SIGNING_PRIVATE_KEY`
   (and password as `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). Never commit it.
3. Push tag `v*` → `release.yml` builds the Windows NSIS installer, signs
   updater artifacts, and publishes a GitHub Release with `latest.json`.
4. The GitHub repository is configured as `baniputrabangsawan/Tidyr`.

The app checks at most once per 24h on startup, only if enabled in Settings;
a manual "Periksa pembaruan" button is in Settings. The only data sent is the
standard HTTPS request for `latest.json`. An invalid signature is rejected.

## Windows code signing (later)

Not required now. Unsigned builds show a SmartScreen warning; users click
"More info → Run anyway". To sign later: buy an EV/OV cert, add the PFX to
GitHub Secrets, and sign in `release.yml` with `signtool` before the Tauri
action uploads artifacts.

## Known limitations

- Similar-photo (perceptual hash), OCR, background watching, treemap,
  WhatsApp-specific rules, cloud, and AI features are out of scope by design.
  Extension points: add a `Detector` producing `plan::Action`s into `Plan`.
- Hardlink detection uses inode ids (Unix); on Windows hardlinked copies are
  treated as normal duplicates (still safe: keeper invariant holds).
- macOS/Linux builds are best-effort (`continue-on-error` in release.yml).
- `trash` crate is intentionally NOT used — quarantine is app-managed.
- `notify` (live folder watching) is out of scope for the MVP.
- Installer size target: Windows NSIS under ~10 MB (release profile uses
  `opt-level="s"`, LTO, `strip`; no UPX). Actual size reported here after the
  first signed release: _TBD_.

## Tests

```sh
cd tidyr
cargo test -p tidyr-core            # 46 unit + 6 acceptance + 1 property suite
```

Acceptance: 5,000-file/500-duplicate fixture — plan summary matches ground
truth; apply→undo restores the exact tree (path + hash). Property:
`prop_apply_undo_is_identity` (20 random trees). Integration tests only use
tempdirs — never real user folders.
