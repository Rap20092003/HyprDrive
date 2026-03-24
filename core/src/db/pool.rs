//! Database connection pool with ADR-001 SQLite pragmas.
//!
//! Configures WAL mode, synchronous=NORMAL, foreign_keys, busy_timeout,
//! mmap_size, and journal_size_limit per the Spacedrive textbook (Ch4).

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::path::Path;
use std::str::FromStr;

/// Create a configured SQLite connection pool.
///
/// Applies all ADR-001 pragmas:
/// - `journal_mode = WAL` (concurrent readers + single writer)
/// - `synchronous = NORMAL` (safe with WAL, 10x faster than FULL)
/// - `foreign_keys = ON` (enforce referential integrity)
/// - `busy_timeout = 5000` (wait 5s before SQLITE_BUSY)
/// - `mmap_size = 256MB` (memory-mapped I/O for reads)
/// - `journal_size_limit = 64MB` (auto-truncate WAL file)
#[tracing::instrument(fields(db_path = %db_path.display()))]
pub async fn create_pool(db_path: &Path) -> Result<SqlitePool, sqlx::Error> {
    let db_url = format!("sqlite:{}?mode=rwc", db_path.display());

    let options = SqliteConnectOptions::from_str(&db_url)?
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(5))
        .pragma("mmap_size", "268435456")
        .pragma("journal_size_limit", "67108864");

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    Ok(pool)
}

/// Run all embedded migrations against the pool (idempotent).
///
/// Tracks applied migrations in an `_applied_migrations` table so
/// each migration is executed exactly once, even across restarts.
#[tracing::instrument(skip(pool))]
pub async fn run_migrations(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    // Ensure the tracking table exists
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS _applied_migrations (
            id INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await?;

    let migrations: &[&str] = &[
        include_str!("../../migrations/001_objects.sql"),
        include_str!("../../migrations/002_locations.sql"),
        include_str!("../../migrations/003_dir_sizes.sql"),
        include_str!("../../migrations/004_metadata.sql"),
        include_str!("../../migrations/005_tags.sql"),
        include_str!("../../migrations/006_virtual_folders.sql"),
        include_str!("../../migrations/007_sync_operations.sql"),
        include_str!("../../migrations/008_file_types.sql"),
        include_str!("../../migrations/009_fts.sql"),
        include_str!("../../migrations/010_fid_column.sql"),
        include_str!("../../migrations/011_cursor_store.sql"),
    ];

    for (i, sql) in migrations.iter().enumerate() {
        let migration_id = (i + 1) as i64;

        // Check if already applied
        let applied: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM _applied_migrations WHERE id = ?")
                .bind(migration_id)
                .fetch_one(pool)
                .await?;

        if applied.0 > 0 {
            continue;
        }

        // Execute migration and record it
        sqlx::raw_sql(sql).execute(pool).await?;
        sqlx::query("INSERT INTO _applied_migrations (id) VALUES (?)")
            .bind(migration_id)
            .execute(pool)
            .await?;
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn test_pool() -> (SqlitePool, TempDir) {
        let dir = TempDir::new().expect("tempdir");
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await.expect("create pool");
        run_migrations(&pool).await.expect("migrations");
        (pool, dir)
    }

    #[tokio::test]
    async fn test_wal_mode() {
        let (pool, _dir) = test_pool().await;
        let row: (String,) = sqlx::query_as("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .expect("pragma");
        assert_eq!(row.0, "wal");
    }

    #[tokio::test]
    async fn test_synchronous_normal() {
        let (pool, _dir) = test_pool().await;
        let row: (i32,) = sqlx::query_as("PRAGMA synchronous")
            .fetch_one(&pool)
            .await
            .expect("pragma");
        // NORMAL = 1
        assert_eq!(row.0, 1);
    }

    #[tokio::test]
    async fn test_journal_size_limit() {
        let (pool, _dir) = test_pool().await;
        let row: (i64,) = sqlx::query_as("PRAGMA journal_size_limit")
            .fetch_one(&pool)
            .await
            .expect("pragma");
        assert_eq!(row.0, 67108864);
    }

    #[tokio::test]
    async fn test_foreign_keys_on() {
        let (pool, _dir) = test_pool().await;
        let row: (i32,) = sqlx::query_as("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .expect("pragma");
        assert_eq!(row.0, 1);
    }

    #[tokio::test]
    async fn test_migrations_run_cleanly() {
        let (pool, _dir) = test_pool().await;
        // Verify objects table exists
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM objects")
            .fetch_one(&pool)
            .await
            .expect("objects table");
        assert_eq!(row.0, 0);
    }

    #[tokio::test]
    async fn test_file_types_seeded() {
        let (pool, _dir) = test_pool().await;
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM file_types")
            .fetch_one(&pool)
            .await
            .expect("file_types");
        assert!(row.0 >= 200, "Expected 200+ file types, got {}", row.0);
    }

    #[tokio::test]
    async fn test_fts5_table_exists() {
        let (pool, _dir) = test_pool().await;
        // Insert a location to test FTS
        sqlx::query("INSERT INTO objects (id, kind, size_bytes) VALUES ('obj1', 'File', 100)")
            .execute(&pool)
            .await
            .expect("insert object");
        sqlx::query(
            "INSERT INTO locations (id, object_id, volume_id, path, name, extension, created_at, modified_at)
             VALUES ('loc1', 'obj1', 'vol1', '/test/hello.txt', 'hello.txt', 'txt', datetime('now'), datetime('now'))"
        )
        .execute(&pool)
        .await
        .expect("insert location");

        // FTS5 MATCH query
        let row: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM files_fts WHERE files_fts MATCH 'hello'")
                .fetch_one(&pool)
                .await
                .expect("fts match");
        assert_eq!(row.0, 1);
    }

    #[tokio::test]
    async fn test_fid_column_exists() {
        let (pool, _dir) = test_pool().await;
        // PRAGMA table_info returns (cid, name, type, notnull, dflt_value, pk)
        let rows: Vec<(i64, String, String, i64, Option<String>, i64)> =
            sqlx::query_as("PRAGMA table_info(locations)")
                .fetch_all(&pool)
                .await
                .expect("table_info");
        let has_fid = rows.iter().any(|r| r.1 == "fid");
        assert!(has_fid, "locations table should have a fid column");
    }

    #[tokio::test]
    async fn test_cursor_store_table_exists() {
        let (pool, _dir) = test_pool().await;
        sqlx::query("INSERT INTO cursor_store (volume_key, cursor_json) VALUES ('C', '{}')")
            .execute(&pool)
            .await
            .expect("insert cursor_store");
        let row: (String,) =
            sqlx::query_as("SELECT cursor_json FROM cursor_store WHERE volume_key = 'C'")
                .fetch_one(&pool)
                .await
                .expect("select cursor_store");
        assert_eq!(row.0, "{}");
    }
}
