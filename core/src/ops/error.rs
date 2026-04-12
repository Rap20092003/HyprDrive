//! Error types for the CQRS operations layer.

/// All errors that can occur when executing a [`super::CoreAction`].
#[derive(Debug, thiserror::Error)]
pub enum OpsError {
    /// Filesystem I/O failure (copy, rename, mkdir, open).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Database query failure.
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    /// Source file or directory does not exist.
    #[error("not found: {path}")]
    NotFound { path: String },

    /// Destination already exists and overwrite was not requested.
    #[error("already exists: {path}")]
    AlreadyExists { path: String },

    /// Caller's session lacks the required permission.
    #[error("permission denied: {reason}")]
    PermissionDenied { reason: String },

    /// Bad input (empty paths, illegal characters in name, etc.).
    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },

    /// `trash` crate returned an error (moved to Recycle Bin / Trash).
    #[error("trash error: {0}")]
    Trash(String),

    /// EXIF parsing error (non-fatal; SmartRename falls back to mtime).
    #[error("EXIF error: {0}")]
    Exif(String),

    /// Serialization/deserialization error for inverse_action JSON.
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    /// A blocking task spawned via `tokio::task::spawn_blocking` panicked.
    #[error("blocking task panicked: {0}")]
    TaskPanicked(String),
}
