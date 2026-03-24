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
/// **Not yet implemented** — returns `UnsupportedPlatform` error at runtime.
/// Callers on macOS should check for this error and inform the user that
/// Phase 5 (getattrlistbulk + FSEvents) is required.
#[cfg_attr(
    target_os = "macos",
    deprecated(
        note = "macOS scanning not implemented (Phase 5). This function always returns Err."
    )
)]
pub fn full_scan(volume: &Path) -> FsIndexerResult<ScanResult> {
    Err(FsIndexerError::UnsupportedPlatform {
        platform: "macOS".to_string(),
        feature: format!("getattrlistbulk scanning for {}", volume.display()),
    })
}
