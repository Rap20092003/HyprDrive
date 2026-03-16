//! Error types for the filesystem indexer.
//!
//! Uses `thiserror` per ADR-006 (library crate).

use crate::types::FilesystemKind;

/// Errors that can occur during filesystem indexing.
#[derive(Debug, thiserror::Error)]
pub enum FsIndexerError {
    /// MFT access denied — typically needs admin/elevated privileges.
    #[error("MFT access denied on volume {volume}: {source}")]
    MftAccess {
        /// Volume path that was inaccessible.
        volume: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// USN journal seek or read failed.
    #[error("USN journal error: {0}")]
    JournalError(std::io::Error),

    /// Permission denied accessing a specific file during enrichment.
    #[error("permission denied: {path}")]
    PermissionDenied {
        /// Path of the inaccessible file.
        path: String,
    },

    /// The filesystem type is not supported for MFT scanning.
    #[error("unsupported filesystem: {kind:?}")]
    UnsupportedFs {
        /// The detected filesystem kind.
        kind: FilesystemKind,
    },

    /// Volume detection failed.
    #[error("failed to detect filesystem on {volume}: {source}")]
    DetectionFailed {
        /// Volume path.
        volume: String,
        /// Underlying error.
        source: std::io::Error,
    },

    /// Path reconstruction failed (broken parent chain).
    #[error("broken parent chain for fid {fid}: parent {parent_fid} not found")]
    BrokenParentChain {
        /// The entry with a missing parent.
        fid: u64,
        /// The parent FRN that was not found.
        parent_fid: u64,
    },

    /// fanotify initialization or watch failed.
    #[error("fanotify error: {source}")]
    FanotifyError {
        /// Underlying I/O error from fanotify.
        source: std::io::Error,
    },

    /// inotify watch limit exhausted.
    #[error("inotify watch limit reached: {current}/{max} watches")]
    InotifyWatchLimit {
        /// Current number of watches.
        current: usize,
        /// Maximum allowed by kernel.
        max: usize,
    },

    /// Pseudo-filesystem detected — should be skipped, not indexed.
    #[error("pseudo-filesystem at {path}: {fs_type}")]
    PseudoFilesystem {
        /// Mount path of the pseudo-filesystem.
        path: String,
        /// Detected filesystem type string.
        fs_type: String,
    },

    /// Generic I/O error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Result type alias for fs-indexer operations.
pub type FsIndexerResult<T> = Result<T, FsIndexerError>;
