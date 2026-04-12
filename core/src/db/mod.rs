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
    add_tags_batch, ancestor_chain, apply_dir_size_delta, build_artifact_waste,
    count_locations_for_object, delete_location_by_fid, delete_location_by_path,
    delete_orphan_objects, duplicate_locations, duplicates_report, fetch_deferred_batch,
    list_files_fast, load_cursor, lookup_location_by_path, pending_hash_count, populate_dir_sizes,
    relocate_location, remove_tags_batch, save_cursor, search_files, stale_files, tags_for_object,
    top_largest_dirs, top_largest_files, type_breakdown, upgrade_deferred_object, upsert_location,
    upsert_locations_batch, upsert_object, upsert_objects_batch, volume_summary,
    wasted_space_report,
};
pub use types::{
    BuildArtifactRow, DeferredObjectRow, DirSizeRow, DuplicateGroupRow, FileRow, FileTypeRow,
    LocationRow, ObjectRow, StaleFileRow, TagRow, TopDirRow, TypeBreakdownRow, VolumeSummary,
    WastedSpaceRow,
};
