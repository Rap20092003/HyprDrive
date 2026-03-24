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
pub use queries::{
    count_locations_for_object, delete_location_by_fid, delete_location_by_path,
    delete_orphan_objects, list_files_fast, load_cursor, relocate_location, save_cursor,
    search_files, upsert_location, upsert_locations_batch, upsert_object, upsert_objects_batch,
};
pub use types::{DirSizeRow, FileRow, FileTypeRow, LocationRow, ObjectRow};
