//! Background hasher — upgrades deferred synthetic ObjectIds to real BLAKE3 content hashes.
//!
//! Runs as a tokio task after the initial scan completes. Fetches batches of
//! deferred objects from the database, hashes them from disk, and atomically
//! upgrades synthetic → real hashes. Also populates the inode cache so
//! subsequent scans get cache hits.

use hyprdrive_core::db::cache::inode;
use hyprdrive_core::db::queries;
use redb::Database;
use sqlx::SqlitePool;
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Configuration for the background hasher.
#[derive(Debug, Clone)]
pub struct BackgroundHasherConfig {
    /// How many deferred objects to fetch per batch.
    pub batch_size: i64,
    /// Delay between batches to avoid starving other I/O.
    pub batch_delay: std::time::Duration,
    /// Volume ID for inode cache key construction.
    pub volume_id: String,
}

impl BackgroundHasherConfig {
    pub fn new(volume_id: String) -> Self {
        Self {
            batch_size: 500,
            batch_delay: std::time::Duration::from_millis(100),
            volume_id,
        }
    }
}

/// Result of a background hashing run.
#[derive(Debug)]
pub struct BackgroundHashResult {
    /// Total objects upgraded from deferred → content.
    pub upgraded: u64,
    /// Objects that failed to hash (file missing, permissions, etc.).
    pub errors: u64,
    /// Objects where old_id == new_id (deferred hash happened to match — shouldn't occur).
    pub unchanged: u64,
}

/// Run the background hasher until all deferred objects are upgraded or cancellation is requested.
///
/// This is meant to be spawned as a tokio task:
/// ```ignore
/// let cancel = CancellationToken::new();
/// let handle = tokio::spawn(run_background_hasher(config, pool, cache, cancel.clone()));
/// // Later: cancel.cancel(); handle.await;
/// ```
pub async fn run_background_hasher(
    config: BackgroundHasherConfig,
    pool: SqlitePool,
    cache: Arc<Database>,
    cancel: CancellationToken,
) -> BackgroundHashResult {
    let mut total_upgraded = 0u64;
    let mut total_errors = 0u64;
    let mut total_unchanged = 0u64;

    tracing::info!(
        batch_size = config.batch_size,
        volume_id = %config.volume_id,
        "background hasher starting"
    );

    loop {
        if cancel.is_cancelled() {
            tracing::info!("background hasher cancelled");
            break;
        }

        // Fetch next batch of deferred objects.
        let batch = match queries::fetch_deferred_batch(&pool, config.batch_size).await {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(error = %e, "failed to fetch deferred batch");
                break;
            }
        };

        if batch.is_empty() {
            tracing::info!(
                upgraded = total_upgraded,
                errors = total_errors,
                "background hasher complete — no more deferred objects"
            );
            break;
        }

        let batch_len = batch.len();
        let mut batch_upgraded = 0u64;
        let mut batch_errors = 0u64;

        for row in &batch {
            if cancel.is_cancelled() {
                break;
            }

            let path = Path::new(&row.path);

            // Hash the file from disk.
            let new_object_id = match crate::hasher::hash_file(path) {
                Ok(id) => id,
                Err(e) => {
                    tracing::debug!(
                        path = %row.path,
                        error = %e,
                        "background hash failed, skipping"
                    );
                    batch_errors += 1;
                    continue;
                }
            };

            let new_id_hex = new_object_id.to_string();

            // Skip if the synthetic and real IDs somehow match (shouldn't happen due to "deferred:" prefix).
            if new_id_hex == row.object_id {
                total_unchanged += 1;
                continue;
            }

            // Upgrade in database: insert real object, re-point locations, delete synthetic.
            match queries::upgrade_deferred_object(&pool, &row.object_id, &new_id_hex, "content")
                .await
            {
                Ok(_) => {
                    batch_upgraded += 1;

                    // Populate inode cache so next scan gets a cache hit.
                    if let Some(fid) = row.fid {
                        if let Ok(fid_u64) = u64::try_from(fid) {
                            let mtime = chrono::NaiveDateTime::parse_from_str(
                                &row.modified_at,
                                "%Y-%m-%d %H:%M:%S",
                            )
                            .map(|dt| dt.and_utc().timestamp())
                            .unwrap_or(0);
                            let size = u64::try_from(row.size_bytes).unwrap_or(0);
                            let cache_key =
                                inode::cache_key_v2(&config.volume_id, fid_u64, mtime, size);
                            if let Err(e) = inode::insert(&cache, &cache_key, &new_id_hex) {
                                tracing::warn!(error = %e, "inode cache insert failed");
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        old_id = %row.object_id,
                        new_id = %new_id_hex,
                        error = %e,
                        "upgrade_deferred_object failed"
                    );
                    batch_errors += 1;
                }
            }
        }

        total_upgraded += batch_upgraded;
        total_errors += batch_errors;

        tracing::info!(
            batch_size = batch_len,
            upgraded = batch_upgraded,
            errors = batch_errors,
            "background hasher batch complete"
        );

        // If no progress was made (all errors), stop to avoid infinite loop
        // on permanently unhashable files.
        if batch_upgraded == 0 {
            tracing::info!(
                total_errors = total_errors,
                "background hasher stopping — no progress in last batch"
            );
            break;
        }

        // Brief delay to avoid monopolizing I/O.
        tokio::time::sleep(config.batch_delay).await;
    }

    BackgroundHashResult {
        upgraded: total_upgraded,
        errors: total_errors,
        unchanged: total_unchanged,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use hyprdrive_core::db::pool::{create_pool, run_migrations};
    use tempfile::TempDir;

    async fn setup() -> (SqlitePool, Arc<Database>, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await.unwrap();
        run_migrations(&pool).await.unwrap();
        let cache = Arc::new(Database::create(dir.path().join("cache.redb")).unwrap());
        (pool, cache, dir)
    }

    #[tokio::test]
    async fn background_hasher_upgrades_deferred() {
        let (pool, cache, dir) = setup().await;

        // Create a real file to hash.
        let file_path = dir.path().join("testfile.txt");
        std::fs::write(&file_path, b"hello background hasher").unwrap();

        // Insert a deferred object + location pointing to the real file.
        let synthetic_id = crate::hasher::synthetic_file_object_id("vol1", 9999, 0, 22);
        let synthetic_hex = synthetic_id.to_string();

        sqlx::query(
            "INSERT INTO objects (id, kind, size_bytes, hash_state, created_at, updated_at)
             VALUES (?1, 'File', 22, 'deferred', datetime('now'), datetime('now'))",
        )
        .bind(&synthetic_hex)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO locations (id, object_id, volume_id, path, name, is_directory,
                                    size_bytes, allocated_bytes, created_at, modified_at)
             VALUES ('loc1', ?1, 'vol1', ?2, 'testfile.txt', 0,
                     22, 4096, datetime('now'), datetime('now'))",
        )
        .bind(&synthetic_hex)
        .bind(file_path.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();

        // Verify it's deferred.
        let pending = queries::pending_hash_count(&pool).await.unwrap();
        assert_eq!(pending, 1);

        // Run background hasher.
        let cancel = CancellationToken::new();
        let config = BackgroundHasherConfig::new("vol1".to_string());
        let result = run_background_hasher(config, pool.clone(), cache, cancel).await;

        assert_eq!(result.upgraded, 1);
        assert_eq!(result.errors, 0);

        // Verify no more deferred objects.
        let pending = queries::pending_hash_count(&pool).await.unwrap();
        assert_eq!(pending, 0);

        // Verify the object now has hash_state = 'content'.
        let row: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM objects WHERE hash_state = 'content'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(row.0 >= 1);
    }

    #[tokio::test]
    async fn background_hasher_handles_missing_files() {
        let (pool, cache, _dir) = setup().await;

        // Insert a deferred object pointing to a nonexistent file.
        let synthetic_id = crate::hasher::synthetic_file_object_id("vol1", 1234, 0, 100);
        let synthetic_hex = synthetic_id.to_string();

        sqlx::query(
            "INSERT INTO objects (id, kind, size_bytes, hash_state, created_at, updated_at)
             VALUES (?1, 'File', 100, 'deferred', datetime('now'), datetime('now'))",
        )
        .bind(&synthetic_hex)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO locations (id, object_id, volume_id, path, name, is_directory,
                                    size_bytes, allocated_bytes, created_at, modified_at)
             VALUES ('loc_missing', ?1, 'vol1', '/nonexistent/file.bin', 'file.bin', 0,
                     100, 4096, datetime('now'), datetime('now'))",
        )
        .bind(&synthetic_hex)
        .execute(&pool)
        .await
        .unwrap();

        let cancel = CancellationToken::new();
        let config = BackgroundHasherConfig::new("vol1".to_string());
        let result = run_background_hasher(config, pool.clone(), cache, cancel).await;

        assert_eq!(result.upgraded, 0);
        assert_eq!(result.errors, 1);

        // Object should still be deferred.
        let pending = queries::pending_hash_count(&pool).await.unwrap();
        assert_eq!(pending, 1);
    }

    #[tokio::test]
    async fn background_hasher_respects_cancellation() {
        let (pool, cache, _dir) = setup().await;
        let cancel = CancellationToken::new();
        cancel.cancel(); // Cancel immediately.

        let config = BackgroundHasherConfig::new("vol1".to_string());
        let result = run_background_hasher(config, pool, cache, cancel).await;

        assert_eq!(result.upgraded, 0);
        assert_eq!(result.errors, 0);
    }
}
