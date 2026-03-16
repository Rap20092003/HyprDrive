//! Platform-native filesystem indexer.
//!
//! Provides fast filesystem enumeration using platform-specific APIs:
//! - **Windows**: NTFS MFT enumeration via `usn-journal-rs`, with `jwalk` fallback
//! - **Linux**: `jwalk` + `inotify` (Phase 4, `io_uring` opt-in future)
//! - **macOS**: `getattrlistbulk` (Phase 5)
//!
//! # Architecture
//!
//! The Windows scanner uses a two-phase approach (validated by Phase -1 spike):
//! 1. **Topology pass**: MFT enumeration → FRN tree (file reference numbers, names, no sizes)
//! 2. **Enrichment pass**: `GetFileInformationByHandleEx(FileStandardInfo)` → sizes
//!
//! This split exists because `usn-journal-rs` `MftEntry` does not expose file sizes.
//!
//! ## Error handling
//!
//! Uses `thiserror` per ADR-006 (this is a library crate).
//!
//! ## Observability
//!
//! All public functions have `#[tracing::instrument]` per ADR-007.
//! Span naming: `scan:{volume}`, `scan:{volume}.topology`, `scan:{volume}.enrich`.

pub mod error;
pub mod platform;
pub mod types;

// Re-export key types at crate root
pub use error::{FsIndexerError, FsIndexerResult};
pub use types::{
    FilesystemKind, FsChange, IndexEntry, LinuxCursor, ScanResult, TopoEntry, UsnCursor,
};

// Re-export platform-specific scanner functions
#[cfg(target_os = "windows")]
pub use platform::windows::scanner::{auto_scan, fallback_scan, full_scan};

#[cfg(target_os = "windows")]
pub use platform::windows::detect::detect_filesystem;

#[cfg(target_os = "windows")]
pub use platform::windows::usn::{poll_changes, read_cursor};

#[cfg(target_os = "windows")]
pub use platform::windows::listener::{CursorStore, ListenerConfig, NoCursorStore, UsnListener};

// Linux platform re-exports
#[cfg(target_os = "linux")]
pub use platform::linux::scanner::{auto_scan, fallback_scan, full_scan};

#[cfg(target_os = "linux")]
pub use platform::linux::detect::{detect_filesystem, MountInfo};

#[cfg(target_os = "linux")]
pub use platform::linux::listener::{LinuxListener, LinuxListenerConfig};
