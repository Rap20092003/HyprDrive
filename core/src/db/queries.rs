//! Database queries for HyprDrive.
//!
//! Key function: `list_files_fast()` — uses keyset pagination with `idx_loc_sort`
//! to list files in a directory in < 5ms at 100k files.

use crate::db::types::FileRow;
use sqlx::{Acquire, SqlitePool};

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
        "INSERT INTO objects (id, kind, mime_type, size_bytes, created_at, updated_at, hash_state)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
           mime_type = COALESCE(excluded.mime_type, objects.mime_type),
           size_bytes = excluded.size_bytes,
           updated_at = excluded.updated_at,
           hash_state = CASE
             WHEN excluded.hash_state = 'content' THEN 'content'
             ELSE objects.hash_state
           END",
    )
    .bind(&row.id)
    .bind(&row.kind)
    .bind(&row.mime_type)
    .bind(row.size_bytes)
    .bind(&row.created_at)
    .bind(&row.updated_at)
    .bind(&row.hash_state)
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
            "INSERT INTO objects (id, kind, mime_type, size_bytes, created_at, updated_at, hash_state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
               mime_type = COALESCE(excluded.mime_type, objects.mime_type),
               size_bytes = excluded.size_bytes,
               updated_at = excluded.updated_at,
               hash_state = CASE
                 WHEN excluded.hash_state = 'content' THEN 'content'
                 ELSE objects.hash_state
               END",
        )
        .bind(&row.id)
        .bind(&row.kind)
        .bind(&row.mime_type)
        .bind(row.size_bytes)
        .bind(&row.created_at)
        .bind(&row.updated_at)
        .bind(&row.hash_state)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Batch upsert locations in a single transaction.
///
/// Temporarily disables foreign key checks on a dedicated connection to avoid
/// FK violations when parent directories haven't been inserted yet (MFT order
/// is arbitrary and entries span multiple batches).
#[tracing::instrument(skip(pool, rows), fields(count = rows.len()))]
pub async fn upsert_locations_batch(
    pool: &SqlitePool,
    rows: &[crate::db::types::LocationRow],
) -> Result<(), sqlx::Error> {
    if rows.is_empty() {
        return Ok(());
    }
    // Acquire a dedicated connection so we can toggle PRAGMA foreign_keys
    // without affecting other pool users. The pragma cannot be changed
    // inside a transaction, so we set it before BEGIN.
    let mut conn = pool.acquire().await?;
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *conn)
        .await?;
    let mut tx = conn.begin().await?;
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
    // Re-enable FK checks on this connection before returning it to the pool.
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Prepare the database for a bulk load by disabling FTS triggers and
/// switching synchronous to OFF for maximum write throughput.
///
/// MUST be paired with [`bulk_load_finish`] after all inserts complete.
pub async fn bulk_load_begin(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    // Drop FTS triggers — they fire per-row and dominate bulk insert time.
    sqlx::raw_sql(
        "DROP TRIGGER IF EXISTS locations_ai;
         DROP TRIGGER IF EXISTS locations_ad;
         DROP TRIGGER IF EXISTS locations_au;",
    )
    .execute(pool)
    .await?;

    // Temporary: disable fsync during bulk load. Safe because a crash
    // during initial scan just means re-scanning, not data corruption.
    sqlx::query("PRAGMA synchronous = OFF")
        .execute(pool)
        .await?;
    tracing::info!("bulk load mode: FTS triggers dropped, synchronous=OFF");
    Ok(())
}

/// Finish a bulk load: rebuild FTS index in one pass and restore triggers + synchronous.
pub async fn bulk_load_finish(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    // Rebuild the FTS5 index in a single pass from the locations table.
    // Much faster than 847K individual trigger fires.
    let rebuild_start = std::time::Instant::now();
    sqlx::query("INSERT INTO files_fts(files_fts) VALUES('rebuild')")
        .execute(pool)
        .await?;
    tracing::info!(
        elapsed_ms = rebuild_start.elapsed().as_millis() as u64,
        "FTS5 index rebuilt"
    );

    // Re-create triggers for incremental updates going forward.
    sqlx::raw_sql(
        "CREATE TRIGGER IF NOT EXISTS locations_ai AFTER INSERT ON locations BEGIN
            INSERT INTO files_fts(rowid, name, path, extension)
            VALUES (new.rowid, new.name, new.path, new.extension);
         END;
         CREATE TRIGGER IF NOT EXISTS locations_ad AFTER DELETE ON locations BEGIN
            INSERT INTO files_fts(files_fts, rowid, name, path, extension)
            VALUES ('delete', old.rowid, old.name, old.path, old.extension);
         END;
         CREATE TRIGGER IF NOT EXISTS locations_au AFTER UPDATE ON locations BEGIN
            INSERT INTO files_fts(files_fts, rowid, name, path, extension)
            VALUES ('delete', old.rowid, old.name, old.path, old.extension);
            INSERT INTO files_fts(rowid, name, path, extension)
            VALUES (new.rowid, new.name, new.path, new.extension);
         END;",
    )
    .execute(pool)
    .await?;

    // Restore safe synchronous mode.
    sqlx::query("PRAGMA synchronous = NORMAL")
        .execute(pool)
        .await?;
    tracing::info!("bulk load complete: FTS triggers restored, synchronous=NORMAL");
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

// ═══ Disk intelligence queries ═══

/// Get aggregate summary statistics for a volume.
pub async fn volume_summary(
    pool: &SqlitePool,
    volume_id: &str,
) -> Result<crate::db::types::VolumeSummary, sqlx::Error> {
    sqlx::query_as(
        "SELECT
            COALESCE(SUM(CASE WHEN is_directory = 0 THEN 1 ELSE 0 END), 0) AS total_files,
            COALESCE(SUM(CASE WHEN is_directory = 1 THEN 1 ELSE 0 END), 0) AS total_dirs,
            COALESCE(SUM(CASE WHEN is_directory = 0 THEN size_bytes ELSE 0 END), 0) AS total_bytes,
            COALESCE(SUM(CASE WHEN is_directory = 0 THEN allocated_bytes ELSE 0 END), 0) AS total_allocated,
            COALESCE(SUM(CASE WHEN is_directory = 0 THEN allocated_bytes - size_bytes ELSE 0 END), 0) AS wasted_bytes
         FROM locations
         WHERE volume_id = ?1",
    )
    .bind(volume_id)
    .fetch_one(pool)
    .await
}

/// Return the N largest files on a volume, ordered by size descending.
pub async fn top_largest_files(
    pool: &SqlitePool,
    volume_id: &str,
    limit: i64,
) -> Result<Vec<crate::db::types::FileRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT l.id AS location_id, l.name, l.extension, l.path,
                l.is_directory, l.size_bytes, l.allocated_bytes,
                l.modified_at, o.id AS object_id, o.kind, o.mime_type
         FROM locations l
         JOIN objects o ON l.object_id = o.id
         WHERE l.volume_id = ?1 AND l.is_directory = 0 AND l.size_bytes > 0
         ORDER BY l.size_bytes DESC
         LIMIT ?2",
    )
    .bind(volume_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Populate the dir_sizes table by aggregating file sizes per directory.
///
/// Two-pass approach:
/// 1. Direct: SUM children's size_bytes and allocated_bytes per parent_id
/// 2. Cumulative: iterative bottom-up rollup adds subdirectory totals
///
/// Called after bulk load completes. Idempotent (clears before inserting).
pub async fn populate_dir_sizes(pool: &SqlitePool, volume_id: &str) -> Result<u64, sqlx::Error> {
    let start = std::time::Instant::now();

    // Clear existing data for this volume's directories
    sqlx::query(
        "DELETE FROM dir_sizes WHERE location_id IN
         (SELECT id FROM locations WHERE volume_id = ?1 AND is_directory = 1)",
    )
    .bind(volume_id)
    .execute(pool)
    .await?;

    // Pass 1: Direct children aggregation per parent directory.
    let result = sqlx::query(
        "INSERT INTO dir_sizes (location_id, file_count, total_bytes, allocated_bytes, cumulative_allocated)
         SELECT
             l.parent_id,
             COALESCE(SUM(CASE WHEN l.is_directory = 0 THEN 1 ELSE 0 END), 0),
             COALESCE(SUM(CASE WHEN l.is_directory = 0 THEN l.size_bytes ELSE 0 END), 0),
             COALESCE(SUM(CASE WHEN l.is_directory = 0 THEN l.allocated_bytes ELSE 0 END), 0),
             COALESCE(SUM(l.allocated_bytes), 0)
         FROM locations l
         WHERE l.volume_id = ?1
           AND l.parent_id IS NOT NULL
           AND l.parent_id IN (SELECT id FROM locations WHERE volume_id = ?1 AND is_directory = 1)
         GROUP BY l.parent_id
         ON CONFLICT(location_id) DO UPDATE SET
             file_count = excluded.file_count,
             total_bytes = excluded.total_bytes,
             allocated_bytes = excluded.allocated_bytes,
             cumulative_allocated = excluded.cumulative_allocated,
             updated_at = datetime('now')",
    )
    .bind(volume_id)
    .execute(pool)
    .await?;

    let direct_count = result.rows_affected();

    // Pass 2: Bottom-up cumulative rollup.
    // Each dir's cumulative = own files' allocated + SUM(children dirs' cumulative).
    // Iterate until convergence. Converges in O(max_depth) passes.
    let mut iterations = 0u32;
    loop {
        iterations += 1;
        let updated = sqlx::query(
            "UPDATE dir_sizes SET cumulative_allocated = (
                SELECT COALESCE(dir_sizes.allocated_bytes, 0) +
                       COALESCE((SELECT SUM(child_ds.cumulative_allocated)
                                 FROM locations child
                                 JOIN dir_sizes child_ds ON child_ds.location_id = child.id
                                 WHERE child.parent_id = dir_sizes.location_id
                                   AND child.is_directory = 1), 0)
             )
             WHERE location_id IN (
                SELECT id FROM locations WHERE volume_id = ?1 AND is_directory = 1
             )",
        )
        .bind(volume_id)
        .execute(pool)
        .await?;

        if updated.rows_affected() == 0 || iterations >= 50 {
            break;
        }
    }

    tracing::info!(
        direct_dirs = direct_count,
        iterations,
        elapsed_ms = start.elapsed().as_millis() as u64,
        "dir_sizes populated"
    );

    Ok(direct_count)
}

/// Return the N largest directories by cumulative allocated bytes.
///
/// Requires `populate_dir_sizes()` to have been called first.
pub async fn top_largest_dirs(
    pool: &SqlitePool,
    volume_id: &str,
    limit: i64,
) -> Result<Vec<crate::db::types::TopDirRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT l.path, l.name, ds.file_count, ds.total_bytes, ds.cumulative_allocated
         FROM dir_sizes ds
         JOIN locations l ON ds.location_id = l.id
         WHERE l.volume_id = ?1
         ORDER BY ds.cumulative_allocated DESC
         LIMIT ?2",
    )
    .bind(volume_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Find directories with the most wasted disk space.
///
/// Wasted = allocated_bytes - total_bytes. High waste ratios indicate
/// NTFS cluster slack, sparse files, or many small files.
pub async fn wasted_space_report(
    pool: &SqlitePool,
    volume_id: &str,
    limit: i64,
) -> Result<Vec<crate::db::types::WastedSpaceRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT l.path, l.name,
                ds.total_bytes,
                ds.allocated_bytes,
                (ds.allocated_bytes - ds.total_bytes) AS wasted_bytes,
                CAST(ds.allocated_bytes AS REAL) / MAX(ds.total_bytes, 1) AS waste_ratio
         FROM dir_sizes ds
         JOIN locations l ON ds.location_id = l.id
         WHERE l.volume_id = ?1 AND ds.total_bytes > 0
         ORDER BY wasted_bytes DESC
         LIMIT ?2",
    )
    .bind(volume_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Find duplicate files — objects that exist at multiple locations.
///
/// Returns groups ordered by total wasted bytes (most wasteful first).
/// Only includes files (not directories) with size > 0.
pub async fn duplicates_report(
    pool: &SqlitePool,
    volume_id: &str,
    limit: i64,
) -> Result<Vec<crate::db::types::DuplicateGroupRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT o.id AS object_id,
                COUNT(*) AS location_count,
                o.size_bytes,
                (COUNT(*) - 1) * o.size_bytes AS wasted_bytes
         FROM locations l
         JOIN objects o ON l.object_id = o.id
         WHERE l.volume_id = ?1 AND l.is_directory = 0 AND o.size_bytes > 0
               AND o.hash_state = 'content'
         GROUP BY o.id
         HAVING COUNT(*) > 1
         ORDER BY wasted_bytes DESC
         LIMIT ?2",
    )
    .bind(volume_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Get all locations for a specific object (duplicate group detail view).
pub async fn duplicate_locations(
    pool: &SqlitePool,
    object_id: &str,
) -> Result<Vec<crate::db::types::FileRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT l.id AS location_id, l.name, l.extension, l.path,
                l.is_directory, l.size_bytes, l.allocated_bytes,
                l.modified_at, o.id AS object_id, o.kind, o.mime_type
         FROM locations l
         JOIN objects o ON l.object_id = o.id
         WHERE o.id = ?1
         ORDER BY l.path",
    )
    .bind(object_id)
    .fetch_all(pool)
    .await
}

// ═══ Disk intelligence — extended queries ═══

/// Break down file types by category, using the file_types seed table.
///
/// Returns categories ordered by total bytes descending. Files with
/// unrecognized extensions are grouped under "Other" / "#9E9E9E".
pub async fn type_breakdown(
    pool: &SqlitePool,
    volume_id: &str,
) -> Result<Vec<crate::db::types::TypeBreakdownRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT COALESCE(ft.category, 'Other') AS category,
                COALESCE(ft.color, '#9E9E9E') AS color,
                COUNT(*) AS file_count,
                COALESCE(SUM(l.size_bytes), 0) AS total_bytes
         FROM locations l
         LEFT JOIN file_types ft ON LOWER(l.extension) = ft.extension
         WHERE l.volume_id = ?1 AND l.is_directory = 0
         GROUP BY category, color
         ORDER BY total_bytes DESC",
    )
    .bind(volume_id)
    .fetch_all(pool)
    .await
}

/// Find the largest files that haven't been modified in `stale_days` days.
pub async fn stale_files(
    pool: &SqlitePool,
    volume_id: &str,
    stale_days: i64,
    limit: i64,
) -> Result<Vec<crate::db::types::StaleFileRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT l.id AS location_id, l.path, l.name, l.extension,
                l.size_bytes, l.modified_at,
                CAST(julianday('now') - julianday(l.modified_at) AS INTEGER) AS days_stale
         FROM locations l
         WHERE l.volume_id = ?1 AND l.is_directory = 0
           AND l.modified_at < datetime('now', '-' || ?2 || ' days')
         ORDER BY l.size_bytes DESC
         LIMIT ?3",
    )
    .bind(volume_id)
    .bind(stale_days)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Find build artifact directories and their total consumed space.
pub async fn build_artifact_waste(
    pool: &SqlitePool,
    volume_id: &str,
    limit: i64,
) -> Result<Vec<crate::db::types::BuildArtifactRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT l.path, l.name,
                COALESCE(ds.total_bytes, 0) AS total_bytes,
                COALESCE(ds.file_count, 0) AS file_count,
                LOWER(l.name) AS pattern
         FROM locations l
         LEFT JOIN dir_sizes ds ON ds.location_id = l.id
         WHERE l.volume_id = ?1 AND l.is_directory = 1
           AND LOWER(l.name) IN (
               'node_modules', 'target', '__pycache__', '.git/objects',
               'dist', '.next', '.nuxt', 'build', '.gradle', '.cache',
               'vendor', '.tox', '.pytest_cache', '.mypy_cache', 'egg-info'
           )
         ORDER BY total_bytes DESC
         LIMIT ?2",
    )
    .bind(volume_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Walk the parent_id chain upward from a location, returning all ancestor IDs.
///
/// Uses a recursive CTE. Results are ordered child-to-root (immediate parent first).
pub async fn ancestor_chain(
    pool: &SqlitePool,
    location_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "WITH RECURSIVE ancestors(id, parent_id, depth) AS (
             SELECT l.parent_id, p.parent_id, 1
             FROM locations l
             JOIN locations p ON l.parent_id = p.id
             WHERE l.id = ?1 AND l.parent_id IS NOT NULL
           UNION ALL
             SELECT a.parent_id, p.parent_id, a.depth + 1
             FROM ancestors a
             JOIN locations p ON a.parent_id = p.id
             WHERE a.parent_id IS NOT NULL
         )
         SELECT id FROM ancestors ORDER BY depth",
    )
    .bind(location_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Apply a delta to a single dir_sizes row (for live bubble-up updates).
///
/// If the row doesn't exist yet, it is created with the delta values.
pub async fn apply_dir_size_delta(
    pool: &SqlitePool,
    location_id: &str,
    file_count_delta: i64,
    bytes_delta: i64,
    allocated_delta: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO dir_sizes (location_id, file_count, total_bytes, allocated_bytes, cumulative_allocated)
         VALUES (?1, MAX(0, ?2), MAX(0, ?3), MAX(0, ?4), MAX(0, ?4))
         ON CONFLICT(location_id) DO UPDATE SET
             file_count = MAX(0, dir_sizes.file_count + ?2),
             total_bytes = MAX(0, dir_sizes.total_bytes + ?3),
             allocated_bytes = MAX(0, dir_sizes.allocated_bytes + ?4),
             cumulative_allocated = MAX(0, dir_sizes.cumulative_allocated + ?4),
             updated_at = datetime('now')",
    )
    .bind(location_id)
    .bind(file_count_delta)
    .bind(bytes_delta)
    .bind(allocated_delta)
    .execute(pool)
    .await?;
    Ok(())
}

// ═══ Deferred hashing queries ═══

/// Count objects still pending real content hashing.
pub async fn pending_hash_count(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM objects WHERE hash_state = 'deferred'")
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// Fetch a batch of deferred objects with their file paths for background hashing.
pub async fn fetch_deferred_batch(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<crate::db::types::DeferredObjectRow>, sqlx::Error> {
    // H4: GROUP BY o.id to avoid returning duplicate rows when one deferred
    // object has multiple locations, preventing wasted re-hashing.
    sqlx::query_as(
        "SELECT o.id AS object_id, MIN(l.path) AS path, l.size_bytes, l.fid, l.modified_at, l.volume_id
         FROM objects o
         JOIN locations l ON l.object_id = o.id
         WHERE o.hash_state = 'deferred' AND l.is_directory = 0
         GROUP BY o.id
         LIMIT ?1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Upgrade a deferred object to its real content hash.
///
/// Atomically: insert new object with real hash, re-point all locations,
/// delete the old synthetic object. Uses a transaction so partial failures
/// leave the DB consistent.
///
/// Returns `Ok(true)` if the upgrade succeeded, `Ok(false)` if the old object
/// was already upgraded (no rows affected).
pub async fn upgrade_deferred_object(
    pool: &SqlitePool,
    old_object_id: &str,
    new_object_id: &str,
) -> Result<bool, sqlx::Error> {
    // H5: Guard against degenerate case where synthetic == real hash.
    // If old == new, the DELETE at the end would cascade-delete all locations.
    if old_object_id == new_object_id {
        tracing::warn!(
            object_id = old_object_id,
            "synthetic and real hash collided — skipping upgrade"
        );
        return Ok(false);
    }

    let mut tx = pool.begin().await?;

    // Insert or update the real content object. hash_state is always 'content'.
    sqlx::query(
        "INSERT INTO objects (id, kind, mime_type, size_bytes, created_at, updated_at, hash_state)
         SELECT ?1, kind, mime_type, size_bytes, created_at, datetime('now'), 'content'
         FROM objects WHERE id = ?2
         ON CONFLICT(id) DO UPDATE SET
           hash_state = 'content',
           updated_at = datetime('now')",
    )
    .bind(new_object_id)
    .bind(old_object_id)
    .execute(&mut *tx)
    .await?;

    // Re-point all locations from old synthetic ID to real content ID
    sqlx::query("UPDATE locations SET object_id = ?1 WHERE object_id = ?2")
        .bind(new_object_id)
        .bind(old_object_id)
        .execute(&mut *tx)
        .await?;

    // Delete the old synthetic object (now orphaned)
    let result = sqlx::query("DELETE FROM objects WHERE id = ?1 AND hash_state = 'deferred'")
        .bind(old_object_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(result.rows_affected() > 0)
}

// ── Phase 9: CQRS Operations Layer additions ─────────────────────────────────

/// Look up a single location by volume and absolute path.
///
/// Returns `None` if no location with that path exists on the given volume.
pub async fn lookup_location_by_path(
    pool: &sqlx::SqlitePool,
    volume_id: &str,
    path: &str,
) -> Result<Option<crate::db::types::LocationRow>, sqlx::Error> {
    sqlx::query_as::<_, crate::db::types::LocationRow>(
        "SELECT id, object_id, volume_id, path, name, extension, parent_id,
                is_directory, size_bytes, allocated_bytes, created_at, modified_at,
                accessed_at, fid
         FROM   locations
         WHERE  volume_id = ?1 AND path = ?2",
    )
    .bind(volume_id)
    .bind(path)
    .fetch_optional(pool)
    .await
}

/// Add a tag to multiple objects in a single transaction.
///
/// Uses `INSERT OR IGNORE` so duplicate junction rows are silently skipped.
/// Returns the number of rows actually inserted.
pub async fn add_tags_batch(
    pool: &sqlx::SqlitePool,
    tag_id: &str,
    object_ids: &[String],
) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let mut inserted: u64 = 0;
    for object_id in object_ids {
        let result = sqlx::query(
            "INSERT OR IGNORE INTO tags_on_objects (tag_id, object_id) VALUES (?1, ?2)",
        )
        .bind(tag_id)
        .bind(object_id)
        .execute(&mut *tx)
        .await?;
        inserted += result.rows_affected();
    }
    tx.commit().await?;
    Ok(inserted)
}

/// Remove a tag from multiple objects in a single transaction.
///
/// Returns the number of junction rows deleted.
pub async fn remove_tags_batch(
    pool: &sqlx::SqlitePool,
    tag_id: &str,
    object_ids: &[String],
) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let mut deleted: u64 = 0;
    for object_id in object_ids {
        let result =
            sqlx::query("DELETE FROM tags_on_objects WHERE tag_id = ?1 AND object_id = ?2")
                .bind(tag_id)
                .bind(object_id)
                .execute(&mut *tx)
                .await?;
        deleted += result.rows_affected();
    }
    tx.commit().await?;
    Ok(deleted)
}

/// Fetch all tags attached to a given object.
pub async fn tags_for_object(
    pool: &sqlx::SqlitePool,
    object_id: &str,
) -> Result<Vec<crate::db::types::TagRow>, sqlx::Error> {
    sqlx::query_as::<_, crate::db::types::TagRow>(
        "SELECT t.id, t.name, t.color, t.parent_id
         FROM   tags t
         JOIN   tags_on_objects tao ON tao.tag_id = t.id
         WHERE  tao.object_id = ?1",
    )
    .bind(object_id)
    .fetch_all(pool)
    .await
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

    /// Create test files WITH size_bytes and allocated_bytes on locations.
    /// Required for disk intelligence queries (the default helper leaves these at 0).
    async fn insert_sized_files(pool: &SqlitePool, parent_id: &str, count: usize) {
        for i in 0..count {
            let obj_id = format!("sobj_{i:05}");
            let loc_id = format!("sloc_{i:05}");
            let name = format!("sized_{i:05}.txt");
            let path = format!("/test/{name}");
            let size = (i as i64 + 1) * 1024; // 1KB, 2KB, 3KB...
            let allocated = ((size / 4096) + 1) * 4096; // Round up to 4K clusters

            sqlx::query(
                "INSERT INTO objects (id, kind, size_bytes, created_at, updated_at)
                 VALUES (?1, 'File', ?2, datetime('now'), datetime('now'))",
            )
            .bind(&obj_id)
            .bind(size)
            .execute(pool)
            .await
            .expect("insert sized object");

            sqlx::query(
                "INSERT INTO locations (id, object_id, volume_id, path, name, extension,
                                        parent_id, is_directory, size_bytes, allocated_bytes,
                                        created_at, modified_at)
                 VALUES (?1, ?2, 'vol1', ?3, ?4, 'txt', ?5, 0, ?6, ?7, datetime('now'), datetime('now'))",
            )
            .bind(&loc_id)
            .bind(&obj_id)
            .bind(&path)
            .bind(&name)
            .bind(parent_id)
            .bind(size)
            .bind(allocated)
            .execute(pool)
            .await
            .expect("insert sized location");
        }
    }

    // ═══ Disk Intelligence Tests ═══

    #[tokio::test]
    async fn test_volume_summary() {
        let (pool, _dir) = setup().await;
        let pid = create_parent_dir(&pool).await;
        insert_sized_files(&pool, &pid, 10).await;

        let s = volume_summary(&pool, "vol1").await.expect("summary");
        assert_eq!(s.total_files, 10);
        assert_eq!(s.total_dirs, 1); // parent dir
        assert!(s.total_bytes > 0, "total_bytes should be > 0");
        assert!(s.total_allocated >= s.total_bytes, "allocated >= logical");
        assert!(s.wasted_bytes >= 0, "wasted should be >= 0");
    }

    #[tokio::test]
    async fn test_top_largest_files() {
        let (pool, _dir) = setup().await;
        let pid = create_parent_dir(&pool).await;
        insert_sized_files(&pool, &pid, 20).await;

        let top5 = top_largest_files(&pool, "vol1", 5).await.expect("top5");
        assert_eq!(top5.len(), 5);
        for i in 1..top5.len() {
            assert!(
                top5[i - 1].size_bytes >= top5[i].size_bytes,
                "not sorted: {} < {}",
                top5[i - 1].size_bytes,
                top5[i].size_bytes
            );
        }
        assert_eq!(top5[0].size_bytes, 20 * 1024);
    }

    #[tokio::test]
    async fn test_populate_dir_sizes() {
        let (pool, _dir) = setup().await;
        let pid = create_parent_dir(&pool).await;
        insert_sized_files(&pool, &pid, 10).await;

        let count = populate_dir_sizes(&pool, "vol1").await.expect("populate");
        assert!(count > 0, "should populate at least 1 dir");

        let ds: Option<crate::db::types::DirSizeRow> =
            sqlx::query_as("SELECT * FROM dir_sizes WHERE location_id = ?1")
                .bind(&pid)
                .fetch_optional(&pool)
                .await
                .expect("query");

        let ds = ds.expect("dir_sizes entry should exist");
        assert_eq!(ds.file_count, 10);
        assert!(ds.total_bytes > 0, "total_bytes > 0");
        assert!(ds.allocated_bytes > 0, "allocated_bytes > 0");
        assert!(
            ds.cumulative_allocated >= ds.allocated_bytes,
            "cumulative >= direct"
        );
    }

    #[tokio::test]
    async fn test_top_largest_dirs() {
        let (pool, _dir) = setup().await;
        let pid = create_parent_dir(&pool).await;
        insert_sized_files(&pool, &pid, 10).await;
        populate_dir_sizes(&pool, "vol1").await.expect("populate");

        let top = top_largest_dirs(&pool, "vol1", 5).await.expect("top");
        assert!(!top.is_empty(), "should have at least 1 dir");
        assert_eq!(top[0].file_count, 10);
        assert!(top[0].cumulative_allocated > 0);
    }

    #[tokio::test]
    async fn test_wasted_space_report() {
        let (pool, _dir) = setup().await;
        let pid = create_parent_dir(&pool).await;
        insert_sized_files(&pool, &pid, 10).await;
        populate_dir_sizes(&pool, "vol1").await.expect("populate");

        let report = wasted_space_report(&pool, "vol1", 10)
            .await
            .expect("report");
        assert!(!report.is_empty(), "should have at least 1 dir with waste");
        for row in &report {
            assert!(row.wasted_bytes >= 0, "wasted_bytes should be >= 0");
            assert!(row.waste_ratio >= 1.0, "waste_ratio should be >= 1.0");
        }
    }

    #[tokio::test]
    async fn test_duplicates_report() {
        let (pool, _dir) = setup().await;

        sqlx::query(
            "INSERT INTO objects (id, kind, size_bytes, created_at, updated_at)
             VALUES ('dup_obj', 'File', 1024, datetime('now'), datetime('now'))",
        )
        .execute(&pool)
        .await
        .expect("obj");

        sqlx::query(
            "INSERT INTO locations (id, object_id, volume_id, path, name, is_directory,
                                    size_bytes, allocated_bytes, created_at, modified_at)
             VALUES ('loc_a', 'dup_obj', 'vol1', '/a/file.txt', 'file.txt', 0,
                     1024, 4096, datetime('now'), datetime('now'))",
        )
        .execute(&pool)
        .await
        .expect("loc_a");

        sqlx::query(
            "INSERT INTO locations (id, object_id, volume_id, path, name, is_directory,
                                    size_bytes, allocated_bytes, created_at, modified_at)
             VALUES ('loc_b', 'dup_obj', 'vol1', '/b/file.txt', 'file.txt', 0,
                     1024, 4096, datetime('now'), datetime('now'))",
        )
        .execute(&pool)
        .await
        .expect("loc_b");

        let dupes = duplicates_report(&pool, "vol1", 10).await.expect("dupes");
        assert_eq!(dupes.len(), 1);
        assert_eq!(dupes[0].location_count, 2);
        assert_eq!(dupes[0].size_bytes, 1024);
        assert_eq!(dupes[0].wasted_bytes, 1024);

        let locs = duplicate_locations(&pool, "dup_obj").await.expect("locs");
        assert_eq!(locs.len(), 2);
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
            hash_state: crate::db::types::hash_state::CONTENT.to_string(),
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

    // ═══ Disk Intelligence Extended Query Tests ═══

    #[tokio::test]
    async fn test_type_breakdown() {
        let (pool, _dir) = setup().await;
        let parent_id = create_parent_dir(&pool).await;

        for (i, ext) in ["jpg", "jpg", "png", "rs", "txt", "xyz"].iter().enumerate() {
            let obj_id = format!("tobj_{i:05}");
            let loc_id = format!("tloc_{i:05}");
            let name = format!("file_{i}.{ext}");
            sqlx::query("INSERT INTO objects (id, kind, size_bytes) VALUES (?1, 'File', ?2)")
                .bind(&obj_id)
                .bind((i as i64 + 1) * 1000)
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query(
                "INSERT INTO locations (id, object_id, volume_id, path, name, extension, parent_id, size_bytes, created_at, modified_at)
                 VALUES (?1, ?2, 'vol1', ?3, ?4, ?5, ?6, ?7, datetime('now'), datetime('now'))",
            )
            .bind(&loc_id)
            .bind(&obj_id)
            .bind(format!("/test/{name}"))
            .bind(&name)
            .bind(ext)
            .bind(&parent_id)
            .bind((i as i64 + 1) * 1000)
            .execute(&pool)
            .await
            .unwrap();
        }

        let rows = type_breakdown(&pool, "vol1").await.unwrap();
        assert!(!rows.is_empty());
        // Image files may be split across sub-categories (different colors per extension)
        let image_total: i64 = rows
            .iter()
            .filter(|r| r.category == "Image")
            .map(|r| r.file_count)
            .sum();
        assert!(
            image_total >= 3,
            "expected >= 3 Image files, got {image_total}"
        ); // 2 jpg + 1 png
    }

    #[tokio::test]
    async fn test_stale_files() {
        let (pool, _dir) = setup().await;
        let parent_id = create_parent_dir(&pool).await;

        sqlx::query(
            "INSERT INTO objects (id, kind, size_bytes) VALUES ('stale_obj', 'File', 50000)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO locations (id, object_id, volume_id, path, name, extension, parent_id, size_bytes, created_at, modified_at)
             VALUES ('stale_loc', 'stale_obj', 'vol1', '/test/old.zip', 'old.zip', 'zip', ?1, 50000, datetime('now'), datetime('now', '-400 days'))",
        )
        .bind(&parent_id)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO objects (id, kind, size_bytes) VALUES ('fresh_obj', 'File', 10000)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO locations (id, object_id, volume_id, path, name, extension, parent_id, size_bytes, created_at, modified_at)
             VALUES ('fresh_loc', 'fresh_obj', 'vol1', '/test/new.zip', 'new.zip', 'zip', ?1, 10000, datetime('now'), datetime('now'))",
        )
        .bind(&parent_id)
        .execute(&pool)
        .await
        .unwrap();

        let stale = stale_files(&pool, "vol1", 365, 10).await.unwrap();
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].name, "old.zip");
        assert!(stale[0].days_stale >= 365);
    }

    #[tokio::test]
    async fn test_build_artifact_waste() {
        let (pool, _dir) = setup().await;

        sqlx::query("INSERT INTO objects (id, kind, size_bytes) VALUES ('nm_obj', 'Directory', 0)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO locations (id, object_id, volume_id, path, name, is_directory, created_at, modified_at)
             VALUES ('nm_loc', 'nm_obj', 'vol1', '/project/node_modules', 'node_modules', 1, datetime('now'), datetime('now'))",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO dir_sizes (location_id, file_count, total_bytes, allocated_bytes, cumulative_allocated)
             VALUES ('nm_loc', 5000, 200000000, 210000000, 210000000)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let artifacts = build_artifact_waste(&pool, "vol1", 10).await.unwrap();
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].name, "node_modules");
        assert_eq!(artifacts[0].total_bytes, 200_000_000);
        assert_eq!(artifacts[0].file_count, 5000);
    }

    #[tokio::test]
    async fn test_ancestor_chain() {
        let (pool, _dir) = setup().await;

        sqlx::query(
            "INSERT INTO objects (id, kind, size_bytes) VALUES ('root_obj', 'Directory', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO locations (id, object_id, volume_id, path, name, is_directory, created_at, modified_at)
             VALUES ('root_loc', 'root_obj', 'vol1', '/root', 'root', 1, datetime('now'), datetime('now'))",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO objects (id, kind, size_bytes) VALUES ('mid_obj', 'Directory', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO locations (id, object_id, volume_id, path, name, is_directory, parent_id, created_at, modified_at)
             VALUES ('mid_loc', 'mid_obj', 'vol1', '/root/mid', 'mid', 1, 'root_loc', datetime('now'), datetime('now'))",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query("INSERT INTO objects (id, kind, size_bytes) VALUES ('leaf_obj', 'File', 1024)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO locations (id, object_id, volume_id, path, name, parent_id, created_at, modified_at)
             VALUES ('leaf_loc', 'leaf_obj', 'vol1', '/root/mid/leaf.txt', 'leaf.txt', 'mid_loc', datetime('now'), datetime('now'))",
        )
        .execute(&pool)
        .await
        .unwrap();

        let chain = ancestor_chain(&pool, "leaf_loc").await.unwrap();
        assert_eq!(chain, vec!["mid_loc".to_string(), "root_loc".to_string()]);
    }

    #[tokio::test]
    async fn test_apply_dir_size_delta() {
        let (pool, _dir) = setup().await;
        let parent_id = create_parent_dir(&pool).await;

        apply_dir_size_delta(&pool, &parent_id, 5, 10240, 12288)
            .await
            .unwrap();

        let row: (i64, i64, i64) = sqlx::query_as(
            "SELECT file_count, total_bytes, cumulative_allocated FROM dir_sizes WHERE location_id = ?1",
        )
        .bind(&parent_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, 5);
        assert_eq!(row.1, 10240);
        assert_eq!(row.2, 12288);

        apply_dir_size_delta(&pool, &parent_id, 3, 4096, 4096)
            .await
            .unwrap();

        let row2: (i64, i64, i64) = sqlx::query_as(
            "SELECT file_count, total_bytes, cumulative_allocated FROM dir_sizes WHERE location_id = ?1",
        )
        .bind(&parent_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row2.0, 8);
        assert_eq!(row2.1, 14336);
        assert_eq!(row2.2, 16384);
    }

    // ── Phase 9: CQRS query tests ─────────────────────────────────────────────

    async fn p9_insert_object(pool: &SqlitePool, id: &str) {
        sqlx::query(
            "INSERT INTO objects (id, kind, size_bytes, hash_state, created_at, updated_at)
             VALUES (?1, 'File', 1024, 'content', datetime('now'), datetime('now'))",
        )
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn p9_insert_location(pool: &SqlitePool, loc_id: &str, obj_id: &str, volume_id: &str, path: &str) {
        let name = std::path::Path::new(path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        sqlx::query(
            "INSERT INTO locations (id, object_id, volume_id, path, name, is_directory,
                                    size_bytes, allocated_bytes, created_at, modified_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, 1024, 4096, datetime('now'), datetime('now'))",
        )
        .bind(loc_id)
        .bind(obj_id)
        .bind(volume_id)
        .bind(path)
        .bind(name)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_lookup_location_by_path() {
        let (pool, _dir) = setup().await;

        p9_insert_object(&pool, "objA").await;
        p9_insert_location(&pool, "locA", "objA", "C", "C:\\Users\\test\\file.txt").await;

        let found = lookup_location_by_path(&pool, "C", "C:\\Users\\test\\file.txt")
            .await
            .unwrap();
        assert!(found.is_some(), "should find the location");
        let loc = found.unwrap();
        assert_eq!(loc.id, "locA");
        assert_eq!(loc.object_id, "objA");
        assert_eq!(loc.path, "C:\\Users\\test\\file.txt");
    }

    #[tokio::test]
    async fn test_lookup_location_by_path_missing() {
        let (pool, _dir) = setup().await;

        let found = lookup_location_by_path(&pool, "C", "C:\\nonexistent\\file.txt")
            .await
            .unwrap();
        assert!(found.is_none(), "should return None for nonexistent path");
    }

    #[tokio::test]
    async fn test_add_tags_batch() {
        let (pool, _dir) = setup().await;

        p9_insert_object(&pool, "obj1").await;
        p9_insert_object(&pool, "obj2").await;
        p9_insert_object(&pool, "obj3").await;
        sqlx::query("INSERT INTO tags (id, name) VALUES ('tag1', 'Important')")
            .execute(&pool)
            .await
            .unwrap();

        let count = add_tags_batch(
            &pool,
            "tag1",
            &["obj1".to_string(), "obj2".to_string(), "obj3".to_string()],
        )
        .await
        .unwrap();
        assert_eq!(count, 3);

        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT object_id FROM tags_on_objects WHERE tag_id = 'tag1' ORDER BY object_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows.len(), 3);

        // Idempotent: re-adding same tag should insert 0
        let count2 = add_tags_batch(&pool, "tag1", &["obj1".to_string()])
            .await
            .unwrap();
        assert_eq!(count2, 0);
    }

    #[tokio::test]
    async fn test_remove_tags_batch() {
        let (pool, _dir) = setup().await;

        p9_insert_object(&pool, "obj1").await;
        p9_insert_object(&pool, "obj2").await;
        sqlx::query("INSERT INTO tags (id, name) VALUES ('tag1', 'Important')")
            .execute(&pool)
            .await
            .unwrap();
        add_tags_batch(&pool, "tag1", &["obj1".to_string(), "obj2".to_string()])
            .await
            .unwrap();

        let removed =
            remove_tags_batch(&pool, "tag1", &["obj1".to_string(), "obj2".to_string()])
                .await
                .unwrap();
        assert_eq!(removed, 2);

        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT object_id FROM tags_on_objects WHERE tag_id = 'tag1'")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(rows.is_empty(), "all tag associations should be removed");
    }

    #[tokio::test]
    async fn test_tags_for_object() {
        let (pool, _dir) = setup().await;

        p9_insert_object(&pool, "obj1").await;
        sqlx::query("INSERT INTO tags (id, name, color) VALUES ('tag1', 'Work', '#FF0000')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO tags (id, name, color) VALUES ('tag2', 'Personal', '#00FF00')")
            .execute(&pool)
            .await
            .unwrap();
        add_tags_batch(&pool, "tag1", &["obj1".to_string()])
            .await
            .unwrap();
        add_tags_batch(&pool, "tag2", &["obj1".to_string()])
            .await
            .unwrap();

        let tags = tags_for_object(&pool, "obj1").await.unwrap();
        assert_eq!(tags.len(), 2);
        let names: Vec<_> = tags.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"Work"));
        assert!(names.contains(&"Personal"));
    }
}
