//! Linux filesystem indexer using jwalk + inotify.
//!
//! Architecture (mirrors Windows two-phase scan):
//! - **Topology pass**: `jwalk` parallel directory walk with inode capture
//! - **Enrichment pass**: batched `lstat()` for sizes and allocated_size
//! - **Delta tracking**: `inotify` real-time filesystem monitoring
//!
//! Future: `io_uring` + `getdents64` fast path behind feature flag.

pub mod detect;
pub mod enrich;
pub mod listener;
pub mod scanner;
pub mod walk;
