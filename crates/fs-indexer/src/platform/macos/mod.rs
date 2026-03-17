//! macOS filesystem indexer — FSEvents + getattrlistbulk.
//!
//! FIXME(phase-5): Implement macOS-native scanning via:
//! - `getattrlistbulk()` for fast metadata enumeration
//! - FSEvents for filesystem change watching
//! - XPC for privileged helper communication

use crate::error::{FsIndexerError, FsIndexerResult};
use crate::types::ScanResult;
use std::path::Path;

/// Perform a full filesystem scan on macOS.
///
/// Not yet implemented — returns an error describing what's needed.
pub fn full_scan(volume: &Path) -> FsIndexerResult<ScanResult> {
    Err(FsIndexerError::UnsupportedPlatform {
        platform: "macOS".to_string(),
        feature: format!("getattrlistbulk scanning for {}", volume.display()),
    })
}
