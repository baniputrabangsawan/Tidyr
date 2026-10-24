# SAFETY.md — Beres safety invariants and their enforcing tests

All invariants are enforced in `beres-core` (never only in the UI), so the
Tauri shell and the CLI inherit them automatically.

## Invariants (from the spec §5.2)

### 1. Nothing is ever permanently deleted, except quarantine purge after retention

- `apply.rs` only ever moves files (into type folders or the quarantine).
  There is no delete-file code path outside `Quarantine::purge_expired`.
- `DeleteEmptyDir` uses `remove_dir`, which fails on non-empty directories.
- Enforced by:
  - `quarantine::tests::purge_by_retention_only_deletes_expired`
  - `quarantine::tests::quarantine_and_restore_roundtrip`
  - `apply::tests::apply_then_undo_restores_tree`
  - acceptance `acceptance_apply_then_undo_restores_tree` (5,000 files)

### 2. Every duplicate group retains at least one file

- `keeper::recommend` errors on empty groups; `validate_keepers` rejects any
  keeper set that keeps zero files; `Plan::set_keeper` re-validates.
- Quarantine actions are only ever created for non-keeper members.
- Enforced by:
  - `keeper::tests::empty_group_is_error`
  - `keeper::tests::validate_keepers_enforces_invariant`
  - `plan::tests::set_keeper_rebuilds_group_actions`
  - property test `prop_apply_undo_is_identity` (quarantine count == groups'
    non-keeper count)

### 3. No action may overwrite an existing file

- `move_file` checks `to.exists()` BEFORE `rename` (rename(2) silently
  replaces on Unix). The cross-volume copy fallback opens the destination
  with `create_new`.
- Organization moves auto-suffix (`name (2).ext`); restores suffix when the
  original path is occupied.
- Enforced by:
  - `quarantine::tests::move_never_overwrites`
  - `quarantine::tests::restore_with_suffix_when_occupied`
  - `rules::tests::conflict_suffixing`

### 4. All operations are journaled before execution and are reversible

- `record_before` runs BEFORE every action; `mark_done` only after success.
  Moves record their `actual_dest` (post-suffix) so undo moves back exactly
  the right file even when siblings share a name. Quarantines record the
  quarantine row id by entry id (never by filename LIKE-match).
- Interrupted (`in_progress`) sessions are listed on startup for rollback.
- Enforced by:
  - `journal::tests::write_ahead_and_recovery`
  - `journal::tests::entries_roundtrip_in_order`
  - acceptance `crash_recovery_offers_rollback`
  - acceptance `acceptance_apply_then_undo_restores_tree`

### 5. Dry-run is the default

- `scan` only reads; `plan::build_plan` is a pure function of scan results.
  The only writes during scan/plan are the hash cache.
- Enforced by construction + `plan::tests::build_plan_matches_ground_truth`.

### 6. No network access except the update check

- `beres-core` has no network dependencies (no http client in Cargo.toml).
  The update check lives in the Tauri shell via `tauri-plugin-updater`,
  at most once per 24h on startup and only when enabled in Settings.

## Protected paths (§5.1)

- `safety::check_scan_root` / `check_action_path` canonicalize first (kills
  `..` and symlink escapes), refuse filesystem roots, the profile root, OS
  locations, and the app's own data dir. The bare `/` prefix is handled by
  the root check, not by prefix-matching everything.
- Enforced by:
  - `safety::tests::refuses_system_dirs`
  - `safety::tests::refuses_dotdot_escape`
  - `safety::tests::refuses_symlink_escape`
  - `safety::tests::refuses_app_data_dir_itself`
  - `safety::tests::allows_normal_folder`

## Other hardening

- No `unwrap()`/`expect()` in non-test code paths that touch user data
  (error paths return `Error` with Indonesian UI strings produced upstream).
- File content is never held fully in memory (streaming hash/copy).
- Logs carry counts and error kinds, not full file paths.
