//! Shared utilities for Windows platform modules.

use crate::error::{FsIndexerError, FsIndexerResult};
use std::path::Path;

/// Extract drive letter from a path like `C:\` → `'C'`.
pub(crate) fn drive_letter_from_path(volume: &Path) -> FsIndexerResult<char> {
    let s = volume.to_string_lossy();
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        Ok(bytes[0] as char)
    } else {
        Err(FsIndexerError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "expected a drive letter path like C:\\, got {}",
                volume.display()
            ),
        )))
    }
}
