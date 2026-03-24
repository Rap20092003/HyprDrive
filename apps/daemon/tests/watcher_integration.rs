//! End-to-end integration tests for the change processor.
//!
//! These tests create real temp files, run them through the pipeline,
//! then simulate FsChange events to verify the full chain works.

use chrono::Utc;
use hyprdrive_core::db::pool::{create_pool, run_migrations};
use hyprdrive_fs_indexer::{FsChange, IndexEntry};
use hyprdrive_object_pipeline::ChangeProcessor;
use redb::Database;
use sqlx::SqlitePool;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

async fn setup() -> (SqlitePool, Arc<Database>, TempDir) {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("test.db");
    let pool = create_pool(&db_path).await.expect("create pool");
    run_migrations(&pool).await.expect("migrations");
    let cache_path = dir.path().join("cache.redb");
    let cache = Arc::new(Database::create(&cache_path).expect("redb"));
    (pool, cache, dir)
}

fn make_entry(fid: u64, full_path: PathBuf, name: &str, size: u64, is_dir: bool) -> IndexEntry {
    IndexEntry {
        fid,
        parent_fid: 0,
        name: OsString::from(name),
        name_lossy: name.to_string(),
        full_path,
        size,
        allocated_size: size,
        is_dir,
        modified_at: Utc::now(),
        attributes: 0,
    }
}

/// Helper to run pipeline and return the result.
async fn pipeline_insert(pool: &SqlitePool, cache: &Arc<Database>, entries: &[IndexEntry]) {
    let config = hyprdrive_object_pipeline::PipelineConfig::new("vol1".to_string());
    let pipeline = hyprdrive_object_pipeline::ObjectPipeline::new_shared(
        config,
        pool.clone(),
        Arc::clone(cache),
    );
    pipeline.process_entries(entries).await.expect("pipeline");
}

#[tokio::test]
async fn test_change_processor_created_e2e() {
    let (pool, cache, dir) = setup().await;

    // Create a real file for the pipeline to hash.
    let test_file = dir.path().join("created.txt");
    std::fs::write(&test_file, b"hello world").expect("write file");

    let processor = Arc::new(ChangeProcessor::new(
        "vol1".to_string(),
        pool.clone(),
        Arc::clone(&cache),
    ));
    let parent_entry = make_entry(1, dir.path().to_path_buf(), "tmp", 0, true);
    processor.seed_fid_map(&[parent_entry]);

    // Simulate a Created event with an enriched entry (full path known).
    let entry = make_entry(42, test_file.clone(), "created.txt", 11, false);
    let stats = processor
        .process_changes(vec![FsChange::Created(entry)])
        .await
        .expect("process");

    assert_eq!(stats.created, 1);
    assert_eq!(stats.errors, 0);

    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM locations WHERE path LIKE '%created.txt%'")
            .fetch_one(&pool)
            .await
            .expect("count");
    assert!(count.0 >= 1, "Expected location for created.txt");
}

#[tokio::test]
async fn test_change_processor_deleted_e2e() {
    let (pool, cache, dir) = setup().await;

    let test_file = dir.path().join("deleteme.txt");
    std::fs::write(&test_file, b"delete me").expect("write file");

    let entry = make_entry(100, test_file.clone(), "deleteme.txt", 9, false);
    pipeline_insert(&pool, &cache, &[entry.clone()]).await;

    // Verify it's in the DB.
    let before: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM locations WHERE fid = 100")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(before.0, 1);

    let processor = Arc::new(ChangeProcessor::new(
        "vol1".to_string(),
        pool.clone(),
        Arc::clone(&cache),
    ));
    processor.seed_fid_map(&[entry]);

    let stats = processor
        .process_changes(vec![FsChange::Deleted {
            fid: 100,
            path: None,
        }])
        .await
        .expect("process");

    assert_eq!(stats.deleted, 1);

    let after: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM locations WHERE fid = 100")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(after.0, 0);

    let obj_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM objects")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(obj_count.0, 0, "Orphaned object should be cleaned up");
}

#[tokio::test]
async fn test_change_processor_moved_e2e() {
    let (pool, cache, dir) = setup().await;

    // Create the file and a destination directory.
    let old_path = dir.path().join("original.txt");
    std::fs::write(&old_path, b"moveme").expect("write file");
    let dest_dir = dir.path().join("subdir");
    std::fs::create_dir(&dest_dir).expect("mkdir");

    // Insert original file and destination dir via pipeline.
    let file_entry = make_entry(200, old_path.clone(), "original.txt", 6, false);
    let dir_entry = make_entry(300, dest_dir.clone(), "subdir", 0, true);
    pipeline_insert(&pool, &cache, &[file_entry.clone(), dir_entry.clone()]).await;

    // Verify fid=200 is stored.
    let before: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM locations WHERE fid = 200")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(before.0, 1);

    // Create change processor and seed fid map with both entries.
    let processor = Arc::new(ChangeProcessor::new(
        "vol1".to_string(),
        pool.clone(),
        Arc::clone(&cache),
    ));
    processor.seed_fid_map(&[file_entry, dir_entry]);

    // Move the file on disk so stat works.
    let new_path = dest_dir.join("renamed.txt");
    std::fs::rename(&old_path, &new_path).expect("rename");

    // Simulate Moved event through process_changes.
    let stats = processor
        .process_changes(vec![FsChange::Moved {
            fid: 200,
            new_parent_fid: 300,
            new_name: "renamed.txt".into(),
        }])
        .await
        .expect("process");

    assert_eq!(stats.moved, 1, "should report 1 moved");
    assert_eq!(stats.errors, 0, "should have no errors");

    // Verify the new path is in the DB.
    let after: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM locations WHERE path LIKE '%renamed.txt%'")
            .fetch_one(&pool)
            .await
            .expect("count");
    assert!(after.0 >= 1, "Expected location with new path");

    // Verify the old path is gone.
    let old: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM locations WHERE path LIKE '%original.txt%'")
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(old.0, 0, "Old location should be removed");
}

#[tokio::test]
async fn test_change_processor_modified_e2e() {
    let (pool, cache, dir) = setup().await;

    let test_file = dir.path().join("modifyme.txt");
    std::fs::write(&test_file, b"original content").expect("write file");

    let entry = make_entry(400, test_file.clone(), "modifyme.txt", 16, false);
    pipeline_insert(&pool, &cache, &[entry.clone()]).await;

    // Modify the file.
    std::fs::write(&test_file, b"modified content that is longer").expect("write file");

    let processor = Arc::new(ChangeProcessor::new(
        "vol1".to_string(),
        pool.clone(),
        Arc::clone(&cache),
    ));
    processor.seed_fid_map(&[entry]);

    let stats = processor
        .process_changes(vec![FsChange::Modified {
            fid: 400,
            new_size: 31,
        }])
        .await
        .expect("process");

    assert_eq!(stats.modified, 1);
    assert_eq!(stats.errors, 0);
}

#[tokio::test]
async fn test_change_processor_full_rescan_flag() {
    let (pool, cache, _dir) = setup().await;
    let processor = Arc::new(ChangeProcessor::new(
        "vol1".to_string(),
        pool,
        Arc::clone(&cache),
    ));

    let stats = processor
        .process_changes(vec![FsChange::FullRescanNeeded {
            volume: PathBuf::from("C:\\"),
            reason: "journal wrapped".to_string(),
        }])
        .await
        .expect("process");

    assert!(stats.rescan_needed);
    assert_eq!(stats.created, 0);
}
