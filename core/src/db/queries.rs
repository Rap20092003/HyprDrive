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
#[tracing::instrument(skip(pool), fields(parent_id, cursor, limit))]
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
#[tracing::instrument(skip(pool), fields(query, limit))]
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

/// Upsert an object row (insert or update on conflict).
///
/// On conflict (same content hash), updates `updated_at` and `size_bytes`.
#[tracing::instrument(skip(pool, row), fields(object_id = %row.id))]
pub async fn upsert_object(
    pool: &SqlitePool,
    row: &crate::db::types::ObjectRow,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO objects (id, kind, mime_type, size_bytes, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET
           mime_type = COALESCE(excluded.mime_type, objects.mime_type),
           size_bytes = excluded.size_bytes,
           updated_at = excluded.updated_at",
    )
    .bind(&row.id)
    .bind(&row.kind)
    .bind(&row.mime_type)
    .bind(row.size_bytes)
    .bind(&row.created_at)
    .bind(&row.updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Upsert a location row (insert or update on conflict).
///
/// On conflict (same volume_id + path), updates metadata fields.
#[tracing::instrument(skip(pool, row), fields(location_id = %row.id, path = %row.path))]
pub async fn upsert_location(
    pool: &SqlitePool,
    row: &crate::db::types::LocationRow,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO locations (id, object_id, volume_id, path, name, extension, parent_id,
                                is_directory, size_bytes, allocated_bytes, created_at, modified_at, accessed_at, fid)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
         ON CONFLICT(volume_id, path) DO UPDATE SET
           object_id = excluded.object_id,
           name = excluded.name,
           extension = excluded.extension,
           is_directory = excluded.is_directory,
           size_bytes = excluded.size_bytes,
           allocated_bytes = excluded.allocated_bytes,
           modified_at = excluded.modified_at,
           accessed_at = excluded.accessed_at,
           fid = excluded.fid",
    )
    .bind(&row.id)
    .bind(&row.object_id)
    .bind(&row.volume_id)
    .bind(&row.path)
    .bind(&row.name)
    .bind(&row.extension)
    .bind(&row.parent_id)
    .bind(row.is_directory)
    .bind(row.size_bytes)
    .bind(row.allocated_bytes)
    .bind(&row.created_at)
    .bind(&row.modified_at)
    .bind(&row.accessed_at)
    .bind(row.fid)
    .execute(pool)
    .await?;
    Ok(())
}

/// Batch upsert objects in a single transaction.
///
/// Wraps all upserts in BEGIN...COMMIT for dramatically better throughput
/// on 100k+ rows (SQLite WAL mode benefits from fewer fsyncs).
#[tracing::instrument(skip(pool, rows), fields(count = rows.len()))]
pub async fn upsert_objects_batch(
    pool: &SqlitePool,
    rows: &[crate::db::types::ObjectRow],
) -> Result<(), sqlx::Error> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    for row in rows {
        sqlx::query(
            "INSERT INTO objects (id, kind, mime_type, size_bytes, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
               mime_type = COALESCE(excluded.mime_type, objects.mime_type),
               size_bytes = excluded.size_bytes,
               updated_at = excluded.updated_at",
        )
        .bind(&row.id)
        .bind(&row.kind)
        .bind(&row.mime_type)
        .bind(row.size_bytes)
        .bind(&row.created_at)
        .bind(&row.updated_at)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Batch upsert locations in a single transaction.
#[tracing::instrument(skip(pool, rows), fields(count = rows.len()))]
pub async fn upsert_locations_batch(
    pool: &SqlitePool,
    rows: &[crate::db::types::LocationRow],
) -> Result<(), sqlx::Error> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    for row in rows {
        sqlx::query(
            "INSERT INTO locations (id, object_id, volume_id, path, name, extension, parent_id,
                                    is_directory, size_bytes, allocated_bytes, created_at, modified_at, accessed_at, fid)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(volume_id, path) DO UPDATE SET
               object_id = excluded.object_id,
               name = excluded.name,
               extension = excluded.extension,
               is_directory = excluded.is_directory,
               size_bytes = excluded.size_bytes,
               allocated_bytes = excluded.allocated_bytes,
               modified_at = excluded.modified_at,
               accessed_at = excluded.accessed_at,
               fid = excluded.fid",
        )
        .bind(&row.id)
        .bind(&row.object_id)
        .bind(&row.volume_id)
        .bind(&row.path)
        .bind(&row.name)
        .bind(&row.extension)
        .bind(&row.parent_id)
        .bind(row.is_directory)
        .bind(row.size_bytes)
        .bind(row.allocated_bytes)
        .bind(&row.created_at)
        .bind(&row.modified_at)
        .bind(&row.accessed_at)
        .bind(row.fid)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Count how many locations reference a given object.
#[tracing::instrument(skip(pool), fields(object_id))]
pub async fn count_locations_for_object(
    pool: &SqlitePool,
    object_id: &str,
) -> Result<i64, sqlx::Error> {
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM locations WHERE object_id = ?1")
        .bind(object_id)
        .fetch_one(pool)
        .await?;
    Ok(count)
}

// ═══ Sub-phase 8.3: Cursor store queries ═══

/// Save (upsert) a watcher cursor for a volume.
#[tracing::instrument(skip(pool, cursor_json), fields(volume_key))]
pub async fn save_cursor(
    pool: &SqlitePool,
    volume_key: &str,
    cursor_json: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO cursor_store (volume_key, cursor_json, updated_at)
         VALUES (?1, ?2, datetime('now'))
         ON CONFLICT(volume_key) DO UPDATE SET
           cursor_json = excluded.cursor_json,
           updated_at = excluded.updated_at",
    )
    .bind(volume_key)
    .bind(cursor_json)
    .execute(pool)
    .await?;
    Ok(())
}

/// Load a watcher cursor for a volume. Returns None if not found.
#[tracing::instrument(skip(pool), fields(volume_key))]
pub async fn load_cursor(
    pool: &SqlitePool,
    volume_key: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT cursor_json FROM cursor_store WHERE volume_key = ?1")
            .bind(volume_key)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|r| r.0))
}

// ═══ Sub-phase 8.4: Delete & orphan cleanup queries ═══

/// Delete a location by (volume_id, fid). Returns the object_id of the deleted row.
#[tracing::instrument(skip(pool), fields(volume_id, fid))]
pub async fn delete_location_by_fid(
    pool: &SqlitePool,
    volume_id: &str,
    fid: i64,
) -> Result<Option<String>, sqlx::Error> {
    // Atomic SELECT+DELETE in a transaction (avoids RETURNING clause compatibility issues).
    let mut tx = pool.begin().await?;

    let row: Option<(String,)> =
        sqlx::query_as("SELECT object_id FROM locations WHERE volume_id = ?1 AND fid = ?2")
            .bind(volume_id)
            .bind(fid)
            .fetch_optional(&mut *tx)
            .await?;

    if let Some(ref r) = row {
        sqlx::query("DELETE FROM locations WHERE volume_id = ?1 AND fid = ?2")
            .bind(volume_id)
            .bind(fid)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(Some(r.0.clone()))
    } else {
        Ok(None)
    }
}

/// Delete a location by (volume_id, path). Fallback when fid is unavailable.
#[tracing::instrument(skip(pool), fields(volume_id, path))]
pub async fn delete_location_by_path(
    pool: &SqlitePool,
    volume_id: &str,
    path: &str,
) -> Result<Option<String>, sqlx::Error> {
    // Atomic SELECT+DELETE in a transaction.
    let mut tx = pool.begin().await?;

    let row: Option<(String,)> =
        sqlx::query_as("SELECT object_id FROM locations WHERE volume_id = ?1 AND path = ?2")
            .bind(volume_id)
            .bind(path)
            .fetch_optional(&mut *tx)
            .await?;

    if let Some(ref r) = row {
        sqlx::query("DELETE FROM locations WHERE volume_id = ?1 AND path = ?2")
            .bind(volume_id)
            .bind(path)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(Some(r.0.clone()))
    } else {
        Ok(None)
    }
}

/// Delete objects that have zero remaining locations. Returns count deleted.
#[tracing::instrument(skip(pool), fields(candidate_count = object_ids.len()))]
pub async fn delete_orphan_objects(
    pool: &SqlitePool,
    object_ids: &[String],
) -> Result<u64, sqlx::Error> {
    if object_ids.is_empty() {
        return Ok(0);
    }
    // Build a single DELETE with dynamic IN (...) and NOT EXISTS subquery.
    let placeholders: Vec<String> = (1..=object_ids.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "DELETE FROM objects WHERE id IN ({}) AND NOT EXISTS (SELECT 1 FROM locations WHERE locations.object_id = objects.id)",
        placeholders.join(", ")
    );
    let mut query = sqlx::query(&sql);
    for oid in object_ids {
        query = query.bind(oid);
    }
    let result = query.execute(pool).await?;
    Ok(result.rows_affected())
}

// ═══ Sub-phase 8.5: Move/rename query ═══

/// Snapshot of location metadata preserved across relocations.
#[derive(sqlx::FromRow)]
struct ExistingLocation {
    object_id: String,
    is_directory: bool,
    size_bytes: i64,
    allocated_bytes: i64,
    created_at: String,
    modified_at: String,
    accessed_at: Option<String>,
}

/// Move a location to a new path. Deletes old row, inserts new with updated path/name/id.
/// Preserves object_id and other metadata. Returns true if a row was moved.
#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip(pool), fields(volume_id, fid, new_path))]
pub async fn relocate_location(
    pool: &SqlitePool,
    volume_id: &str,
    fid: i64,
    new_location_id: &str,
    new_path: &str,
    new_name: &str,
    new_extension: Option<&str>,
    new_parent_id: Option<&str>,
) -> Result<bool, sqlx::Error> {
    // All operations in a single transaction to avoid TOCTOU.
    let mut tx = pool.begin().await?;

    // Fetch existing row by (volume_id, fid)
    let existing: Option<ExistingLocation> = sqlx::query_as(
        "SELECT object_id, is_directory, size_bytes, allocated_bytes,
                created_at, modified_at, accessed_at
         FROM locations WHERE volume_id = ?1 AND fid = ?2",
    )
    .bind(volume_id)
    .bind(fid)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(row) = existing else {
        return Ok(false);
    };

    // Delete old location
    sqlx::query("DELETE FROM locations WHERE volume_id = ?1 AND fid = ?2")
        .bind(volume_id)
        .bind(fid)
        .execute(&mut *tx)
        .await?;

    // Insert new location preserving metadata
    sqlx::query(
        "INSERT INTO locations (id, object_id, volume_id, path, name, extension, parent_id,
                                is_directory, size_bytes, allocated_bytes, created_at, modified_at, accessed_at, fid)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
    )
    .bind(new_location_id)
    .bind(&row.object_id)
    .bind(volume_id)
    .bind(new_path)
    .bind(new_name)
    .bind(new_extension)
    .bind(new_parent_id)
    .bind(row.is_directory)
    .bind(row.size_bytes)
    .bind(row.allocated_bytes)
    .bind(&row.created_at)
    .bind(&row.modified_at)
    .bind(&row.accessed_at)
    .bind(fid)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(true)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
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

    // ═══ Upsert Tests ═══

    fn make_object_row(id: &str, kind: &str, size: i64) -> crate::db::types::ObjectRow {
        crate::db::types::ObjectRow {
            id: id.to_string(),
            kind: kind.to_string(),
            mime_type: None,
            size_bytes: size,
            created_at: "2026-01-01 00:00:00".to_string(),
            updated_at: "2026-01-01 00:00:00".to_string(),
        }
    }

    fn make_location_row(
        id: &str,
        object_id: &str,
        volume_id: &str,
        path: &str,
        name: &str,
    ) -> crate::db::types::LocationRow {
        crate::db::types::LocationRow {
            id: id.to_string(),
            object_id: object_id.to_string(),
            volume_id: volume_id.to_string(),
            path: path.to_string(),
            name: name.to_string(),
            extension: None,
            parent_id: None,
            is_directory: false,
            size_bytes: 1024,
            allocated_bytes: 4096,
            created_at: "2026-01-01 00:00:00".to_string(),
            modified_at: "2026-01-01 00:00:00".to_string(),
            accessed_at: None,
            fid: None,
        }
    }

    #[tokio::test]
    async fn test_upsert_object_insert_new() {
        let (pool, _dir) = setup().await;
        let row = make_object_row("obj_new", "File", 1024);
        upsert_object(&pool, &row).await.expect("upsert");

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM objects WHERE id = 'obj_new'")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count.0, 1);
    }

    #[tokio::test]
    async fn test_upsert_object_idempotent() {
        let (pool, _dir) = setup().await;
        let row = make_object_row("obj_idem", "File", 1024);
        upsert_object(&pool, &row).await.expect("first");
        upsert_object(&pool, &row).await.expect("second");

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM objects WHERE id = 'obj_idem'")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count.0, 1);
    }

    #[tokio::test]
    async fn test_upsert_object_updates_metadata() {
        let (pool, _dir) = setup().await;
        let mut row = make_object_row("obj_upd", "File", 1024);
        upsert_object(&pool, &row).await.expect("insert");

        row.size_bytes = 2048;
        row.mime_type = Some("text/plain".to_string());
        row.updated_at = "2026-06-01 00:00:00".to_string();
        upsert_object(&pool, &row).await.expect("update");

        let (size, mime): (i64, Option<String>) =
            sqlx::query_as("SELECT size_bytes, mime_type FROM objects WHERE id = 'obj_upd'")
                .fetch_one(&pool)
                .await
                .expect("fetch");
        assert_eq!(size, 2048);
        assert_eq!(mime.as_deref(), Some("text/plain"));
    }

    #[tokio::test]
    async fn test_upsert_location_insert_new() {
        let (pool, _dir) = setup().await;
        let obj = make_object_row("obj_loc", "File", 100);
        upsert_object(&pool, &obj).await.expect("obj");

        let loc = make_location_row("loc1", "obj_loc", "vol1", "/a/b.txt", "b.txt");
        upsert_location(&pool, &loc).await.expect("loc");

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM locations WHERE id = 'loc1'")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count.0, 1);
    }

    #[tokio::test]
    async fn test_upsert_location_idempotent() {
        let (pool, _dir) = setup().await;
        let obj = make_object_row("obj_li", "File", 100);
        upsert_object(&pool, &obj).await.expect("obj");

        let loc = make_location_row("loc_i", "obj_li", "vol1", "/x.txt", "x.txt");
        upsert_location(&pool, &loc).await.expect("first");
        upsert_location(&pool, &loc).await.expect("second");

        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM locations WHERE volume_id = 'vol1' AND path = '/x.txt'",
        )
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(count.0, 1);
    }

    #[tokio::test]
    async fn test_count_locations_for_object() {
        let (pool, _dir) = setup().await;
        let obj = make_object_row("obj_dup", "File", 100);
        upsert_object(&pool, &obj).await.expect("obj");

        let loc1 = make_location_row("loc_a", "obj_dup", "vol1", "/a.txt", "a.txt");
        let loc2 = make_location_row("loc_b", "obj_dup", "vol1", "/b.txt", "b.txt");
        upsert_location(&pool, &loc1).await.expect("loc1");
        upsert_location(&pool, &loc2).await.expect("loc2");

        let count = count_locations_for_object(&pool, "obj_dup")
            .await
            .expect("count");
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_upsert_objects_batch_empty() {
        let (pool, _dir) = setup().await;
        upsert_objects_batch(&pool, &[]).await.expect("empty batch");
    }

    #[tokio::test]
    async fn test_upsert_objects_batch_multiple() {
        let (pool, _dir) = setup().await;
        let rows = vec![
            make_object_row("batch_1", "File", 100),
            make_object_row("batch_2", "File", 200),
            make_object_row("batch_3", "Directory", 0),
        ];
        upsert_objects_batch(&pool, &rows).await.expect("batch");

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM objects")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count.0, 3);
    }

    #[tokio::test]
    async fn test_upsert_objects_batch_idempotent() {
        let (pool, _dir) = setup().await;
        let rows = vec![
            make_object_row("bi_1", "File", 100),
            make_object_row("bi_2", "File", 200),
        ];
        upsert_objects_batch(&pool, &rows).await.expect("first");
        upsert_objects_batch(&pool, &rows).await.expect("second");

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM objects")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count.0, 2);
    }

    // ═══ Sub-phase 8.1: fid upsert test ═══

    #[tokio::test]
    async fn test_upsert_location_with_fid() {
        let (pool, _dir) = setup().await;
        let obj = make_object_row("obj_fid", "File", 100);
        upsert_object(&pool, &obj).await.expect("obj");

        let mut loc = make_location_row("loc_fid", "obj_fid", "vol1", "/fid.txt", "fid.txt");
        loc.fid = Some(12345);
        upsert_location(&pool, &loc).await.expect("loc");

        let row: (Option<i64>,) = sqlx::query_as("SELECT fid FROM locations WHERE id = 'loc_fid'")
            .fetch_one(&pool)
            .await
            .expect("fid");
        assert_eq!(row.0, Some(12345));
    }

    // ═══ Sub-phase 8.3: Cursor store query tests ═══

    #[tokio::test]
    async fn test_save_and_load_cursor() {
        let (pool, _dir) = setup().await;
        save_cursor(&pool, "vol_C", r#"{"usn":42}"#)
            .await
            .expect("save");
        let loaded = load_cursor(&pool, "vol_C").await.expect("load");
        assert_eq!(loaded.as_deref(), Some(r#"{"usn":42}"#));
    }

    #[tokio::test]
    async fn test_load_cursor_missing() {
        let (pool, _dir) = setup().await;
        let loaded = load_cursor(&pool, "nonexistent").await.expect("load");
        assert!(loaded.is_none());
    }

    #[tokio::test]
    async fn test_save_cursor_overwrites() {
        let (pool, _dir) = setup().await;
        save_cursor(&pool, "vol_D", r#"{"usn":1}"#)
            .await
            .expect("save1");
        save_cursor(&pool, "vol_D", r#"{"usn":99}"#)
            .await
            .expect("save2");
        let loaded = load_cursor(&pool, "vol_D").await.expect("load");
        assert_eq!(loaded.as_deref(), Some(r#"{"usn":99}"#));
    }

    // ═══ Sub-phase 8.4: Delete & orphan cleanup tests ═══

    /// Helper: insert an object + location with fid.
    async fn insert_object_with_fid_location(
        pool: &SqlitePool,
        obj_id: &str,
        loc_id: &str,
        vol: &str,
        path: &str,
        name: &str,
        fid: i64,
    ) {
        let obj = make_object_row(obj_id, "File", 1024);
        upsert_object(pool, &obj).await.expect("obj");
        let mut loc = make_location_row(loc_id, obj_id, vol, path, name);
        loc.fid = Some(fid);
        upsert_location(pool, &loc).await.expect("loc");
    }

    #[tokio::test]
    async fn test_delete_location_by_fid() {
        let (pool, _dir) = setup().await;
        insert_object_with_fid_location(
            &pool, "obj_d1", "loc_d1", "vol1", "/d1.txt", "d1.txt", 100,
        )
        .await;

        let result = delete_location_by_fid(&pool, "vol1", 100)
            .await
            .expect("delete");
        assert_eq!(result.as_deref(), Some("obj_d1"));

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM locations WHERE id = 'loc_d1'")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count.0, 0);
    }

    #[tokio::test]
    async fn test_delete_location_by_fid_nonexistent() {
        let (pool, _dir) = setup().await;
        let result = delete_location_by_fid(&pool, "vol1", 99999)
            .await
            .expect("delete");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_delete_location_by_path() {
        let (pool, _dir) = setup().await;
        insert_object_with_fid_location(
            &pool, "obj_dp", "loc_dp", "vol1", "/dp.txt", "dp.txt", 200,
        )
        .await;

        let result = delete_location_by_path(&pool, "vol1", "/dp.txt")
            .await
            .expect("delete");
        assert_eq!(result.as_deref(), Some("obj_dp"));

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM locations WHERE id = 'loc_dp'")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count.0, 0);
    }

    #[tokio::test]
    async fn test_delete_orphan_objects_removes_unreferenced() {
        let (pool, _dir) = setup().await;
        insert_object_with_fid_location(
            &pool,
            "obj_orph",
            "loc_orph",
            "vol1",
            "/orph.txt",
            "orph.txt",
            300,
        )
        .await;

        // Delete the location, leaving the object orphaned
        delete_location_by_fid(&pool, "vol1", 300)
            .await
            .expect("delete loc");

        let deleted = delete_orphan_objects(&pool, &["obj_orph".to_string()])
            .await
            .expect("orphan");
        assert_eq!(deleted, 1);

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM objects WHERE id = 'obj_orph'")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count.0, 0);
    }

    #[tokio::test]
    async fn test_delete_orphan_objects_keeps_referenced() {
        let (pool, _dir) = setup().await;
        // Object with TWO locations
        let obj = make_object_row("obj_ref", "File", 1024);
        upsert_object(&pool, &obj).await.expect("obj");

        let mut loc1 = make_location_row("loc_r1", "obj_ref", "vol1", "/r1.txt", "r1.txt");
        loc1.fid = Some(400);
        upsert_location(&pool, &loc1).await.expect("loc1");

        let mut loc2 = make_location_row("loc_r2", "obj_ref", "vol1", "/r2.txt", "r2.txt");
        loc2.fid = Some(401);
        upsert_location(&pool, &loc2).await.expect("loc2");

        // Delete one location
        delete_location_by_fid(&pool, "vol1", 400)
            .await
            .expect("delete");

        // Orphan cleanup should keep the object (still has loc_r2)
        let deleted = delete_orphan_objects(&pool, &["obj_ref".to_string()])
            .await
            .expect("orphan");
        assert_eq!(deleted, 0);

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM objects WHERE id = 'obj_ref'")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count.0, 1);
    }

    // ═══ Sub-phase 8.5: Relocate location tests ═══

    #[tokio::test]
    async fn test_relocate_location() {
        let (pool, _dir) = setup().await;
        insert_object_with_fid_location(
            &pool, "obj_mv", "loc_mv", "vol1", "/old.txt", "old.txt", 500,
        )
        .await;

        let moved = relocate_location(
            &pool,
            "vol1",
            500,
            "loc_mv_new",
            "/new.txt",
            "new.txt",
            Some("txt"),
            None,
        )
        .await
        .expect("relocate");
        assert!(moved);

        // Old location gone
        let old: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM locations WHERE id = 'loc_mv'")
            .fetch_one(&pool)
            .await
            .expect("old");
        assert_eq!(old.0, 0);

        // New location exists
        let new: (String,) = sqlx::query_as("SELECT path FROM locations WHERE id = 'loc_mv_new'")
            .fetch_one(&pool)
            .await
            .expect("new");
        assert_eq!(new.0, "/new.txt");
    }

    #[tokio::test]
    async fn test_relocate_location_nonexistent() {
        let (pool, _dir) = setup().await;
        let moved = relocate_location(
            &pool, "vol1", 99999, "loc_new", "/new.txt", "new.txt", None, None,
        )
        .await
        .expect("relocate");
        assert!(!moved);
    }

    #[tokio::test]
    async fn test_relocate_location_preserves_object_id() {
        let (pool, _dir) = setup().await;
        insert_object_with_fid_location(
            &pool,
            "obj_pres",
            "loc_pres",
            "vol1",
            "/pres.txt",
            "pres.txt",
            600,
        )
        .await;

        relocate_location(
            &pool,
            "vol1",
            600,
            "loc_pres_new",
            "/moved.txt",
            "moved.txt",
            Some("txt"),
            None,
        )
        .await
        .expect("relocate");

        let (oid,): (String,) =
            sqlx::query_as("SELECT object_id FROM locations WHERE id = 'loc_pres_new'")
                .fetch_one(&pool)
                .await
                .expect("oid");
        assert_eq!(oid, "obj_pres");
    }
}
