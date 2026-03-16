//! Error types for the dedup engine.

/// Errors that can occur during duplicate detection.
#[derive(Debug, thiserror::Error)]
pub enum DeduplicateError {
    /// IO error reading a file.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Error hashing a file.
    #[error("hash error: {0}")]
    HashError(String),

    /// Error processing an image for perceptual hashing.
    #[error("image error: {0}")]
    ImageError(String),
}

/// Result type for dedup operations.
pub type DeduplicateResult<T> = Result<T, DeduplicateError>;
