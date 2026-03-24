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
    let cache = redb::Database::create(dir.path().join("cache.redb")).unwrap();

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
    assert_eq!(
        stats.hashed, 3,
        "all 3 files hashed (cache miss); same ObjectId for dups, DB upsert deduplicates"
    );
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
    let cache2 = redb::Database::create(dir.path().join("cache2.redb")).unwrap();
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

/// All-directory batch: no file I/O, only synthetic ObjectIds.
#[tokio::test]
async fn pipeline_all_directories() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test.db");
    let pool = hyprdrive_core::db::pool::create_pool(&db_path)
        .await
        .unwrap();
    hyprdrive_core::db::pool::run_migrations(&pool)
        .await
        .unwrap();
    let cache = redb::Database::create(dir.path().join("cache.redb")).unwrap();

    let entries = vec![
        make_entry(dir.path().join("dir_a"), 0, true),
        make_entry(dir.path().join("dir_b"), 0, true),
        make_entry(dir.path().join("dir_c"), 0, true),
    ];

    let config = hyprdrive_object_pipeline::PipelineConfig::new("vol".to_string());
    let pipeline = hyprdrive_object_pipeline::ObjectPipeline::new(config, pool.clone(), cache);
    let stats = pipeline.process_entries(&entries).await.unwrap();

    assert_eq!(stats.total, 3);
    assert_eq!(stats.hashed, 0, "directories are not hashed");
    assert_eq!(stats.directories, 3);

    let (obj_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM objects WHERE kind = 'Directory'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        obj_count, 3,
        "each directory gets a unique synthetic ObjectId"
    );

    pool.close().await;
}

/// Unicode file paths are handled without panics or data loss.
#[tokio::test]
async fn pipeline_unicode_paths() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test.db");
    let pool = hyprdrive_core::db::pool::create_pool(&db_path)
        .await
        .unwrap();
    hyprdrive_core::db::pool::run_migrations(&pool)
        .await
        .unwrap();
    let cache = redb::Database::create(dir.path().join("cache.redb")).unwrap();

    // Create files with Unicode names (CJK, emoji, accented chars).
    let file_cjk = dir.path().join("文件.txt");
    let file_emoji = dir.path().join("🚀rocket.txt");
    let file_accent = dir.path().join("café.txt");
    std::fs::write(&file_cjk, b"chinese").unwrap();
    std::fs::write(&file_emoji, b"emoji").unwrap();
    std::fs::write(&file_accent, b"french").unwrap();

    let entries = vec![
        make_entry(file_cjk, 7, false),
        make_entry(file_emoji, 5, false),
        make_entry(file_accent, 6, false),
    ];

    let config = hyprdrive_object_pipeline::PipelineConfig::new("vol".to_string());
    let pipeline = hyprdrive_object_pipeline::ObjectPipeline::new(config, pool.clone(), cache);
    let stats = pipeline.process_entries(&entries).await.unwrap();

    assert_eq!(stats.total, 3);
    assert_eq!(stats.errors, 0, "no errors on Unicode paths");

    let (loc_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM locations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(loc_count, 3, "all Unicode-named files persisted");

    pool.close().await;
}

/// Large batch (10K entries) to verify batch chunking works.
#[tokio::test]
async fn pipeline_large_batch_10k() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test.db");
    let pool = hyprdrive_core::db::pool::create_pool(&db_path)
        .await
        .unwrap();
    hyprdrive_core::db::pool::run_migrations(&pool)
        .await
        .unwrap();
    let cache = redb::Database::create(dir.path().join("cache.redb")).unwrap();

    // Create 10K directory entries (no file I/O needed).
    let entries: Vec<_> = (0..10_000)
        .map(|i| make_entry(dir.path().join(format!("dir_{i:05}")), 0, true))
        .collect();

    let mut config = hyprdrive_object_pipeline::PipelineConfig::new("vol".to_string());
    config.batch_size = 2000; // Force multiple batches.
    let pipeline = hyprdrive_object_pipeline::ObjectPipeline::new(config, pool.clone(), cache);
    let stats = pipeline.process_entries(&entries).await.unwrap();

    assert_eq!(stats.total, 10_000);
    assert_eq!(stats.directories, 10_000);
    assert_eq!(stats.errors, 0);

    let (loc_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM locations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(loc_count, 10_000, "all 10K entries persisted");

    pool.close().await;
}

/// Parent-child relationships are resolved via fid→LocationId mapping.
#[tokio::test]
async fn pipeline_parent_id_resolved() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test.db");
    let pool = hyprdrive_core::db::pool::create_pool(&db_path)
        .await
        .unwrap();
    hyprdrive_core::db::pool::run_migrations(&pool)
        .await
        .unwrap();
    let cache = redb::Database::create(dir.path().join("cache.redb")).unwrap();

    // Create a parent directory and a child file.
    let parent_dir = dir.path().join("parent");
    std::fs::create_dir_all(&parent_dir).unwrap();
    let child_file = parent_dir.join("child.txt");
    std::fs::write(&child_file, b"child content").unwrap();

    let parent_entry = hyprdrive_fs_indexer::IndexEntry {
        fid: 100,
        parent_fid: 0,
        name: std::ffi::OsString::from("parent"),
        name_lossy: "parent".to_string(),
        full_path: parent_dir.clone(),
        size: 0,
        allocated_size: 0,
        is_dir: true,
        modified_at: chrono::Utc::now(),
        attributes: 0,
    };

    let child_entry = hyprdrive_fs_indexer::IndexEntry {
        fid: 200,
        parent_fid: 100, // Points to parent's fid.
        name: std::ffi::OsString::from("child.txt"),
        name_lossy: "child.txt".to_string(),
        full_path: child_file,
        size: 13,
        allocated_size: 4096,
        is_dir: false,
        modified_at: chrono::Utc::now(),
        attributes: 0,
    };

    let entries = vec![parent_entry, child_entry];

    let config = hyprdrive_object_pipeline::PipelineConfig::new("vol".to_string());
    let pipeline = hyprdrive_object_pipeline::ObjectPipeline::new(config, pool.clone(), cache);
    pipeline.process_entries(&entries).await.unwrap();

    // Verify parent_id is set on the child location.
    let rows: Vec<(Option<String>,)> =
        sqlx::query_as("SELECT parent_id FROM locations WHERE name = 'child.txt'")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].0.is_some(), "child's parent_id should be set");

    // The parent_id should match the parent directory's location_id.
    let (parent_loc_id,): (String,) =
        sqlx::query_as("SELECT id FROM locations WHERE name = 'parent'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(rows[0].0.as_deref(), Some(parent_loc_id.as_str()));

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
    let cache = redb::Database::create(dir.path().join("cache.redb")).unwrap();

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
