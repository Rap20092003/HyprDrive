//! Integration test: full pipeline from scan entries to populated database.
//!
//! Validates that the daemon wiring (Phase 7) correctly:
//! 1. Hashes files via BLAKE3
//! 2. Upserts ObjectRow + LocationRow into SQLite
//! 3. Deduplicates identical content into a single object
//! 4. Survives idempotent reruns without creating duplicates

use chrono::Utc;
use std::ffi::OsString;
use std::path::PathBuf;
use tempfile::TempDir;

fn make_entry(path: PathBuf, size: u64, is_dir: bool) -> hyprdrive_fs_indexer::IndexEntry {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1000);
    hyprdrive_fs_indexer::IndexEntry {
        fid: COUNTER.fetch_add(1, Ordering::Relaxed),
        parent_fid: 0,
        name: OsString::from(path.file_name().unwrap_or_default()),
        name_lossy: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        full_path: path,
        size,
        allocated_size: size.next_multiple_of(4096),
        is_dir,
        modified_at: Utc::now(),
        attributes: 0,
    }
}

/// Full pipeline integration: create temp files, run pipeline, verify DB state.
#[tokio::test]
async fn pipeline_populates_objects_and_locations() {
    let dir = TempDir::new().unwrap();

    // Set up SQLite
    let db_path = dir.path().join("test.db");
    let pool = hyprdrive_core::db::pool::create_pool(&db_path)
        .await
        .unwrap();
    hyprdrive_core::db::pool::run_migrations(&pool)
        .await
        .unwrap();

    // Set up redb cache
    let cache =
        redb::Database::create(dir.path().join("cache.redb")).unwrap();

    // Create test files with known content
    let file_a = dir.path().join("alpha.txt");
    let file_b = dir.path().join("beta.txt");
    let file_dup = dir.path().join("alpha_copy.txt");
    std::fs::write(&file_a, b"content alpha").unwrap();
    std::fs::write(&file_b, b"content beta").unwrap();
    std::fs::write(&file_dup, b"content alpha").unwrap(); // duplicate of alpha

    let entries = vec![
        make_entry(file_a, 13, false),
        make_entry(file_b, 12, false),
        make_entry(file_dup, 13, false),
        make_entry(dir.path().join("subdir"), 0, true), // directory
    ];

    // Run pipeline
    let config = hyprdrive_object_pipeline::PipelineConfig::new("test_vol".to_string());
    let pipeline = hyprdrive_object_pipeline::ObjectPipeline::new(config, pool.clone(), cache);
    let stats = pipeline.process_entries(&entries).await.unwrap();

    // Verify stats
    assert_eq!(stats.total, 4);
    assert_eq!(stats.hashed, 3, "all 3 files hashed (cache miss); same ObjectId for dups, DB upsert deduplicates");
    assert_eq!(stats.skipped, 0);

    // Verify DB: 3 objects (alpha, beta, subdir) — alpha and alpha_copy share one
    let (obj_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM objects")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(obj_count, 3, "alpha+dup share 1 object, beta=1, subdir=1");

    // Verify DB: 4 locations (one per entry)
    let (loc_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM locations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(loc_count, 4, "one location per input entry");

    // Verify idempotency: re-run should not create duplicates
    let config2 = hyprdrive_object_pipeline::PipelineConfig::new("test_vol".to_string());
    let cache2 =
        redb::Database::create(dir.path().join("cache2.redb")).unwrap();
    let pipeline2 = hyprdrive_object_pipeline::ObjectPipeline::new(config2, pool.clone(), cache2);
    pipeline2.process_entries(&entries).await.unwrap();

    let (obj_count2,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM objects")
        .fetch_one(&pool)
        .await
        .unwrap();
    let (loc_count2,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM locations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(obj_count2, 3, "idempotent: same object count after rerun");
    assert_eq!(loc_count2, 4, "idempotent: same location count after rerun");

    // Verify directory entry has correct kind
    let (dir_kind,): (String,) =
        sqlx::query_as("SELECT kind FROM objects WHERE kind = 'Directory'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(dir_kind, "Directory");

    pool.close().await;
}

/// Empty pipeline produces no rows and zero stats.
#[tokio::test]
async fn pipeline_empty_input_is_noop() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test.db");
    let pool = hyprdrive_core::db::pool::create_pool(&db_path)
        .await
        .unwrap();
    hyprdrive_core::db::pool::run_migrations(&pool)
        .await
        .unwrap();
    let cache =
        redb::Database::create(dir.path().join("cache.redb")).unwrap();

    let config = hyprdrive_object_pipeline::PipelineConfig::new("vol".to_string());
    let pipeline = hyprdrive_object_pipeline::ObjectPipeline::new(config, pool.clone(), cache);
    let stats = pipeline.process_entries(&[]).await.unwrap();

    assert_eq!(stats.total, 0);
    assert_eq!(stats.hashed, 0);

    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM objects")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);

    pool.close().await;
}
