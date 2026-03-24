//! SQLite-backed cursor store for persisting watcher cursors across restarts.
//!
//! Bridges the sync `CursorStore` trait (called from `spawn_blocking` poll loop)
//! to async SQLite queries via `Handle::current().block_on()`.
//!
//! This is platform-agnostic — each platform serializes its own cursor type
//! (e.g. `UsnCursor`, `LinuxCursor`) to JSON before calling `save`.

// Currently only constructed on Windows (wired in main.rs cfg(windows) block).
// Will be used on Linux/macOS once their listeners are implemented.
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use hyprdrive_core::db::queries;
use hyprdrive_fs_indexer::CursorStore;
use sqlx::SqlitePool;
use tokio::runtime::Handle;

/// A `CursorStore` implementation backed by SQLite.
///
/// The watcher's `poll_loop` runs inside `spawn_blocking`, so it calls
/// sync methods. We bridge to async SQLite via `Handle::current().block_on()`.
pub struct SqliteCursorStore {
    pool: SqlitePool,
}

impl SqliteCursorStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

impl CursorStore for SqliteCursorStore {
    fn save(
        &self,
        volume_key: &str,
        cursor_json: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Handle::current().block_on(queries::save_cursor(&self.pool, volume_key, cursor_json))?;
        Ok(())
    }

    fn load(
        &self,
        volume_key: &str,
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        let json = Handle::current().block_on(queries::load_cursor(&self.pool, volume_key))?;
        Ok(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyprdrive_core::db::pool::{create_pool, run_migrations};
    use tempfile::TempDir;

    async fn setup() -> (SqlitePool, TempDir) {
        let dir = TempDir::new().expect("tempdir");
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await.expect("create pool");
        run_migrations(&pool).await.expect("migrations");
        (pool, dir)
    }

    /// Tests run the store methods inside `spawn_blocking` to simulate
    /// the real usage where `poll_loop` runs in a blocking thread.
    #[tokio::test]
    async fn test_sqlite_cursor_store_roundtrip() {
        let (pool, _dir) = setup().await;
        let store = std::sync::Arc::new(SqliteCursorStore::new(pool));
        let s = store.clone();
        tokio::task::spawn_blocking(move || {
            let json = r#"{"journal_id":42,"next_usn":12345}"#;
            s.save("C", json).expect("save");
            let loaded = s.load("C").expect("load");
            assert_eq!(loaded, Some(json.to_string()));
        })
        .await
        .expect("spawn_blocking");
    }

    #[tokio::test]
    async fn test_sqlite_cursor_store_overwrite() {
        let (pool, _dir) = setup().await;
        let store = std::sync::Arc::new(SqliteCursorStore::new(pool));
        let s = store.clone();
        tokio::task::spawn_blocking(move || {
            s.save("D", r#"{"journal_id":1,"next_usn":100}"#)
                .expect("save1");
            s.save("D", r#"{"journal_id":1,"next_usn":999}"#)
                .expect("save2");
            let loaded = s.load("D").expect("load");
            assert_eq!(
                loaded,
                Some(r#"{"journal_id":1,"next_usn":999}"#.to_string())
            );
        })
        .await
        .expect("spawn_blocking");
    }

    #[tokio::test]
    async fn test_sqlite_cursor_store_missing() {
        let (pool, _dir) = setup().await;
        let store = std::sync::Arc::new(SqliteCursorStore::new(pool));
        let s = store.clone();
        tokio::task::spawn_blocking(move || {
            let loaded = s.load("Z").expect("load");
            assert!(loaded.is_none());
        })
        .await
        .expect("spawn_blocking");
    }
}
