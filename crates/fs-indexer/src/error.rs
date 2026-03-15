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

    /// Generic I/O error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Result type alias for fs-indexer operations.
pub type FsIndexerResult<T> = Result<T, FsIndexerError>;
