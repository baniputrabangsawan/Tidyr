//! Error types for beres-core.

use std::path::PathBuf;
use thiserror::Error;

/// All errors produced by beres-core. User-facing text is produced by the
/// UI layer (Indonesian strings); these variants carry structured context.
#[derive(Debug, Error)]
pub enum Error {
    #[error("path is protected and must not be modified: {0}")]
    ProtectedPath(PathBuf),

    #[error("path is outside the allowed scan root: {0}")]
    OutsideScanRoot(PathBuf),

    #[error("file changed since scan, skipped: {0}")]
    ChangedSinceScan(PathBuf),

    #[error("destination already exists, refusing to overwrite: {0}")]
    WouldOverwrite(PathBuf),

    #[error("duplicate group would keep zero files")]
    EmptyKeeper,

    #[error("action failed: {0}")]
    ActionFailed(String),

    #[error("journal error: {0}")]
    Journal(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Crate-local result alias.
pub type Result<T> = std::result::Result<T, Error>;
