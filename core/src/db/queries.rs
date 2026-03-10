//! Database queries for HyprDrive.
//!
//! Key function: `list_files_fast()` — uses keyset pagination with `idx_loc_sort`
//! to list files in a directory in < 5ms at 100k files.

use crate::db::types::FileRow;
use sqlx::SqlitePool;

/// List files in a directory using keyset (cursor) pagination.
///
/// - `parent_id`: parent location ID (None for root)
/// - `cursor`: last name from previous page (None for first page)
/// - `limit`: max rows to return
///
/// Uses `idx_loc_sort(parent_id, name)` for O(log n) seeks.
/// Returns `FileRow` — a COMPUTED struct from JOIN objects + locations.
pub async fn list_files_fast(
    pool: &SqlitePool,
    parent_id: Option<&str>,
    cursor: Option<&str>,
    limit: i64,
) -> Result<Vec<FileRow>, sqlx::Error> {
    let rows = match (parent_id, cursor) {
        (Some(pid), Some(cur)) => {
            sqlx::query_as::<_, FileRow>(
                "SELECT l.id AS location_id, l.name, l.extension, l.path,
                        l.is_directory, l.size_bytes, l.allocated_bytes,
                        l.modified_at, o.id AS object_id, o.kind, o.mime_type
                 FROM locations l
                 JOIN objects o ON l.object_id = o.id
                 WHERE l.parent_id = ?1 AND l.name > ?2
                 ORDER BY l.name ASC
                 LIMIT ?3",
            )
            .bind(pid)
            .bind(cur)
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
        (Some(pid), None) => {
            sqlx::query_as::<_, FileRow>(
                "SELECT l.id AS location_id, l.name, l.extension, l.path,
                        l.is_directory, l.size_bytes, l.allocated_bytes,
                        l.modified_at, o.id AS object_id, o.kind, o.mime_type
                 FROM locations l
                 JOIN objects o ON l.object_id = o.id
                 WHERE l.parent_id = ?1
                 ORDER BY l.name ASC
                 LIMIT ?2",
            )
            .bind(pid)
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
        (None, Some(cur)) => {
            sqlx::query_as::<_, FileRow>(
                "SELECT l.id AS location_id, l.name, l.extension, l.path,
                        l.is_directory, l.size_bytes, l.allocated_bytes,
                        l.modified_at, o.id AS object_id, o.kind, o.mime_type
                 FROM locations l
                 JOIN objects o ON l.object_id = o.id
                 WHERE l.parent_id IS NULL AND l.name > ?1
                 ORDER BY l.name ASC
                 LIMIT ?2",
            )
            .bind(cur)
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
        (None, None) => {
            sqlx::query_as::<_, FileRow>(
                "SELECT l.id AS location_id, l.name, l.extension, l.path,
                        l.is_directory, l.size_bytes, l.allocated_bytes,
                        l.modified_at, o.id AS object_id, o.kind, o.mime_type
                 FROM locations l
                 JOIN objects o ON l.object_id = o.id
                 WHERE l.parent_id IS NULL
                 ORDER BY l.name ASC
                 LIMIT ?1",
            )
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
    };

    Ok(rows)
}

/// Full-text search on file names using FTS5.
pub async fn search_files(
    pool: &SqlitePool,
    query: &str,
    limit: i64,
) -> Result<Vec<FileRow>, sqlx::Error> {
    let rows = sqlx::query_as::<_, FileRow>(
        "SELECT l.id AS location_id, l.name, l.extension, l.path,
                l.is_directory, l.size_bytes, l.allocated_bytes,
                l.modified_at, o.id AS object_id, o.kind, o.mime_type
         FROM files_fts f
         JOIN locations l ON f.rowid = l.rowid
         JOIN objects o ON l.object_id = o.id
         WHERE files_fts MATCH ?1
         LIMIT ?2",
    )
    .bind(query)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::{create_pool, run_migrations};
    use tempfile::TempDir;

    async fn setup() -> (SqlitePool, TempDir) {
        let dir = TempDir::new().expect("tempdir");
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await.expect("create pool");
        run_migrations(&pool).await.expect("migrations");
        (pool, dir)
    }

    /// Create a parent directory location so child files can reference it via FK.
    async fn create_parent_dir(pool: &SqlitePool) -> String {
        let parent_id = "parent_dir_001";
        sqlx::query(
            "INSERT INTO objects (id, kind, size_bytes) VALUES ('obj_parent', 'Directory', 0)",
        )
        .execute(pool)
        .await
        .expect("insert parent object");
        sqlx::query(
            "INSERT INTO locations (id, object_id, volume_id, path, name, is_directory, created_at, modified_at)
             VALUES (?1, 'obj_parent', 'vol1', '/test', 'test', 1, datetime('now'), datetime('now'))"
        )
        .bind(parent_id)
        .execute(pool)
        .await
        .expect("insert parent location");
        parent_id.to_string()
    }

    async fn insert_test_files(pool: &SqlitePool, parent_id: &str, count: usize) {
        for i in 0..count {
            let obj_id = format!("obj_{i:05}");
            let loc_id = format!("loc_{i:05}");
            let name = format!("file_{i:05}.txt");
            let path = format!("/test/{name}");

            sqlx::query("INSERT INTO objects (id, kind, size_bytes) VALUES (?1, 'File', ?2)")
                .bind(&obj_id)
                .bind(i as i64 * 1024)
                .execute(pool)
                .await
                .expect("insert object");

            sqlx::query(
                "INSERT INTO locations (id, object_id, volume_id, path, name, extension, parent_id, created_at, modified_at)
                 VALUES (?1, ?2, 'vol1', ?3, ?4, 'txt', ?5, datetime('now'), datetime('now'))"
            )
            .bind(&loc_id)
            .bind(&obj_id)
            .bind(&path)
            .bind(&name)
            .bind(parent_id)
            .execute(pool)
            .await
            .expect("insert location");
        }
    }

    #[tokio::test]
    async fn test_list_files_returns_correct_count() {
        let (pool, _dir) = setup().await;
        let pid = create_parent_dir(&pool).await;
        insert_test_files(&pool, &pid, 100).await;

        let files = list_files_fast(&pool, Some(&pid), None, 100)
            .await
            .expect("list");
        assert_eq!(files.len(), 100);
    }

    #[tokio::test]
    async fn test_list_files_ordered_by_name() {
        let (pool, _dir) = setup().await;
        let pid = create_parent_dir(&pool).await;
        insert_test_files(&pool, &pid, 10).await;

        let files = list_files_fast(&pool, Some(&pid), None, 10)
            .await
            .expect("list");

        for i in 1..files.len() {
            assert!(
                files[i - 1].name <= files[i].name,
                "Not sorted: {} > {}",
                files[i - 1].name,
                files[i].name
            );
        }
    }

    #[tokio::test]
    async fn test_cursor_pagination() {
        let (pool, _dir) = setup().await;
        let pid = create_parent_dir(&pool).await;
        insert_test_files(&pool, &pid, 100).await;

        // First page: 50 items
        let page1 = list_files_fast(&pool, Some(&pid), None, 50)
            .await
            .expect("page1");
        assert_eq!(page1.len(), 50);

        // Second page: next 50
        let cursor = &page1.last().expect("last").name;
        let page2 = list_files_fast(&pool, Some(&pid), Some(cursor), 50)
            .await
            .expect("page2");
        assert_eq!(page2.len(), 50);

        // No overlap
        let last_p1 = &page1.last().expect("last").name;
        let first_p2 = &page2.first().expect("first").name;
        assert!(last_p1 < first_p2, "Pages overlap");
    }

    #[tokio::test]
    async fn test_empty_parent_returns_empty() {
        let (pool, _dir) = setup().await;
        let files = list_files_fast(&pool, Some("nonexistent"), None, 50)
            .await
            .expect("list");
        assert!(files.is_empty());
    }

    #[tokio::test]
    async fn test_explain_uses_index() {
        let (pool, _dir) = setup().await;
        // EXPLAIN QUERY PLAN returns (id, parent, notused, detail)
        let rows: Vec<(i32, i32, i32, String)> = sqlx::query_as(
            "EXPLAIN QUERY PLAN
             SELECT l.id, l.name, l.extension, l.path,
                    l.is_directory, l.size_bytes, l.allocated_bytes,
                    l.modified_at, o.id, o.kind, o.mime_type
             FROM locations l
             JOIN objects o ON l.object_id = o.id
             WHERE l.parent_id = 'root'
             ORDER BY l.name ASC
             LIMIT 50",
        )
        .fetch_all(&pool)
        .await
        .expect("explain");

        let plan = rows
            .iter()
            .map(|r| r.3.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            plan.contains("idx_loc_sort") || plan.contains("idx_loc_parent"),
            "Expected index usage in EXPLAIN: {plan}"
        );
    }

    #[tokio::test]
    async fn test_search_files_fts5() {
        let (pool, _dir) = setup().await;

        // Insert a file
        sqlx::query("INSERT INTO objects (id, kind, size_bytes) VALUES ('obj1', 'File', 100)")
            .execute(&pool)
            .await
            .expect("insert obj");
        sqlx::query(
            "INSERT INTO locations (id, object_id, volume_id, path, name, extension, created_at, modified_at)
             VALUES ('loc1', 'obj1', 'vol1', '/docs/readme.md', 'readme.md', 'md', datetime('now'), datetime('now'))"
        )
        .execute(&pool)
        .await
        .expect("insert loc");

        let results = search_files(&pool, "readme", 10).await.expect("search");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "readme.md");
    }
}
