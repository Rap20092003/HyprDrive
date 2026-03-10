//! Database layer for HyprDrive.
//!
//! Provides SQLite connection pool with ADR-001 pragmas,
//! embedded migrations (9 tables), queries with keyset pagination,
//! and redb caches for hot-path lookups.

pub mod cache;
pub mod pool;
pub mod queries;
pub mod types;

// Re-export key types
pub use cache::{DirSizeRecord, ThumbRecord};
pub use pool::{create_pool, run_migrations};
pub use queries::{list_files_fast, search_files};
pub use types::{DirSizeRow, FileRow, FileTypeRow, LocationRow, ObjectRow};
