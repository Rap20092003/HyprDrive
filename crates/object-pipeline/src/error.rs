//! Error types for the object pipeline.

/// Errors that can occur during pipeline processing.
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    /// IO error reading or accessing a file.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Error hashing a file via the dedup engine.
    #[error("hash error: {0}")]
    Hash(String),

    /// Database error during upsert.
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    /// Cache error from redb.
    #[error("cache error: {0}")]
    Cache(#[from] Box<redb::Error>),

    /// Invalid entry that cannot be processed.
    #[error("invalid entry: {0}")]
    InvalidEntry(String),
}

/// Result type for pipeline operations.
pub type PipelineResult<T> = Result<T, PipelineError>;
