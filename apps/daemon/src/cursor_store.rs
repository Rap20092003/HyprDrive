//! SQLite-backed cursor store for persisting USN journal positions.
//!
//! Bridges the sync `CursorStore` trait (called from `spawn_blocking` poll loop)
//! to async SQLite queries via `Handle::current().block_on()`.

use hyprdrive_core::db::queries;
use hyprdrive_fs_indexer::{CursorStore, UsnCursor};
use sqlx::SqlitePool;
use std::error::Error;
use tokio::runtime::Handle;

/// A `CursorStore` implementation backed by SQLite.
///
/// The USN listener's `poll_loop` runs inside `spawn_blocking`, so it calls
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
        cursor: &UsnCursor,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let json = serde_json::to_string(cursor)?;
        Handle::current().block_on(queries::save_cursor(&self.pool, volume_key, &json))?;
        Ok(())
    }

    fn load(&self, volume_key: &str) -> Result<Option<UsnCursor>, Box<dyn Error + Send + Sync>> {
        let json = Handle::current().block_on(queries::load_cursor(&self.pool, volume_key))?;
        match json {
            Some(j) => Ok(Some(serde_json::from_str(&j)?)),
            None => Ok(None),
        }
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
            let cursor = UsnCursor {
                journal_id: 42,
                next_usn: 12345,
            };
            s.save("C", &cursor).expect("save");
            let loaded = s.load("C").expect("load");
            assert_eq!(loaded, Some(cursor));
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
            let c1 = UsnCursor {
                journal_id: 1,
                next_usn: 100,
            };
            let c2 = UsnCursor {
                journal_id: 1,
                next_usn: 999,
            };
            s.save("D", &c1).expect("save1");
            s.save("D", &c2).expect("save2");
            let loaded = s.load("D").expect("load");
            assert_eq!(loaded, Some(c2));
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
