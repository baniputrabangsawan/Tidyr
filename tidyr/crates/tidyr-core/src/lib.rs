//! tidyr-core: all business logic for the Tidyr desktop app.
//!
//! This crate has zero UI/Tauri dependencies so every module is testable
//! without a GUI. Safety invariants enforced here:
//!
//! 1. Nothing is ever permanently deleted, except quarantine purge after retention.
//! 2. Every duplicate group retains at least one file.
//! 3. No action may overwrite an existing file.
//! 4. All operations are journaled before execution and are reversible.
//! 5. Dry-run is the default: scan/plan never touch the filesystem except reads + cache.
//! 6. No network access (update check lives in the Tauri shell, not here).

pub mod apply;
pub mod cache;
pub mod classify;
pub mod dedupe;
pub mod error;
pub mod journal;
pub mod keeper;
pub mod plan;
pub mod quarantine;
pub mod rules;
pub mod safety;
pub mod scan;

pub use error::{Error, Result};
