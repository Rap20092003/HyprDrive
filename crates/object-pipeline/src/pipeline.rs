//! Object pipeline orchestrator.
//!
//! Coordinates: IndexEntry → cache check → BLAKE3 hash → ObjectId → DB upsert.
//! Emits `PipelineBatchComplete` events for observability.

use crate::error::PipelineResult;
use crate::hasher::hash_entries_batch;
use chrono::Utc;
use hyprdrive_core::db::queries;
use hyprdrive_core::db::types::{LocationRow, ObjectRow};
use hyprdrive_core::domain::events::PipelineBatchComplete;
use hyprdrive_core::domain::id::ObjectId;
use hyprdrive_fs_indexer::types::IndexEntry;
use redb::Database;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

/// Sentinel value for "no parent" in `IndexEntry.parent_fid`.
pub const NO_PARENT_FID: u64 = 0;

/// Configuration for the object pipeline.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Volume identifier for this scan batch.
    pub volume_id: String,
    /// Number of entries to process in each DB batch.
    pub batch_size: usize,
    /// Whether to skip directory entries entirely.
    pub skip_directories: bool,
    /// Whether to detect MIME types from file extensions.
    pub mime_detection: bool,
}

impl PipelineConfig {
    /// Create a new config with sensible defaults.
    pub fn new(volume_id: String) -> Self {
        Self {
            volume_id,
            batch_size: 20_000,
            skip_directories: false,
            mime_detection: true,
        }
    }
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self::new(String::new())
    }
}

/// The object pipeline: transforms IndexEntries into persisted objects + locations.
pub struct ObjectPipeline {
    config: PipelineConfig,
    pool: SqlitePool,
    cache: Arc<Database>,
}

impl ObjectPipeline {
    /// Create a new pipeline with the given configuration.
    pub fn new(config: PipelineConfig, pool: SqlitePool, cache: Database) -> Self {
        Self {
            config,
            pool,
            cache: Arc::new(cache),
        }
    }

    /// Create a new pipeline with a shared cache (Arc<Database>).
    pub fn new_shared(config: PipelineConfig, pool: SqlitePool, cache: Arc<Database>) -> Self {
        Self {
            config,
            pool,
            cache,
        }
    }

    /// Process a batch of IndexEntries through the full pipeline.
    ///
    /// 1. Hash entries (cache-aware, rayon-parallel)
    /// 2. Build ObjectRow + LocationRow structs
    /// 3. Batch upsert into SQLite
    /// 4. Return completion stats
    #[tracing::instrument(skip_all, fields(
        volume_id = %self.config.volume_id,
        total_entries = entries.len()
    ))]
    pub async fn process_entries(
        &self,
        entries: &[IndexEntry],
    ) -> PipelineResult<PipelineBatchComplete> {
        let start = Instant::now();
        let total = entries.len();

        if entries.is_empty() {
            return Ok(PipelineBatchComplete {
                total: 0,
                hashed: 0,
                cached: 0,
                skipped: 0,
                errors: 0,
                directories: 0,
                zero_byte: 0,
                elapsed: start.elapsed(),
            });
        }

        // Filter directories if configured.
        let mut working_entries: Vec<&IndexEntry> = if self.config.skip_directories {
            entries.iter().filter(|e| !e.is_dir).collect()
        } else {
            entries.iter().collect()
        };

        // Sort by path depth so parent directories are inserted before children.
        // This prevents FK violations on locations.parent_id when entries span
        // multiple batches (MFT order is arbitrary, not tree-order).
        working_entries.sort_by_key(|e| e.full_path.components().count());

        // Build fid → LocationId map AND fid → LocationId cache for reuse.
        // location_id_for_entry() is a BLAKE3 hash — computing it once here
        // and reusing it in the inner loop avoids 847K redundant hashes.
        let fid_to_location_id: HashMap<u64, String> = entries
            .iter()
            .map(|e| {
                let loc_id = location_id_for_entry(&self.config.volume_id, &e.full_path);
                (e.fid, loc_id)
            })
            .collect();

        // Process in batches to limit memory and transaction size.
        let mut total_hashed = 0usize;
        let mut total_cached = 0usize;
        let mut total_skipped = 0usize;
        let mut total_errors = 0usize;
        let mut total_directories = 0usize;
        let mut total_zero_byte = 0usize;

        let num_batches = working_entries.len().div_ceil(self.config.batch_size);
        tracing::info!(
            entries = working_entries.len(),
            batches = num_batches,
            "starting pipeline"
        );

        for (batch_idx, chunk) in working_entries.chunks(self.config.batch_size).enumerate() {
            tracing::info!(
                batch = batch_idx + 1,
                of = num_batches,
                chunk_size = chunk.len(),
                "processing batch"
            );

            // Collect owned entries for the hasher (it expects a slice of IndexEntry).
            let chunk_owned: Vec<IndexEntry> = chunk.iter().map(|e| (*e).clone()).collect();

            // Count per-kind entries in this chunk.
            for e in &chunk_owned {
                if e.is_dir {
                    total_directories += 1;
                } else if e.size == 0 {
                    total_zero_byte += 1;
                }
            }

            // Hash the batch.
            let hash_result = hash_entries_batch(&chunk_owned, &self.cache, &self.config.volume_id);

            total_hashed += hash_result.hashed;
            total_cached += hash_result.cache_hits;
            total_skipped += hash_result.skipped;
            total_errors += hash_result.errors.len();

            // Build ObjectRows and LocationRows from hash results.
            let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
            let mut object_rows: Vec<ObjectRow> = Vec::with_capacity(hash_result.results.len());
            let mut location_rows: Vec<LocationRow> = Vec::with_capacity(hash_result.results.len());

            for hr in &hash_result.results {
                let entry = &chunk_owned[hr.index];
                let object_id_hex = hr.object_id.to_string();
                let kind = if entry.is_dir { "Directory" } else { "File" };

                let mime_type = if self.config.mime_detection && !entry.is_dir {
                    mime_from_extension(&entry.full_path)
                } else {
                    None
                };

                object_rows.push(ObjectRow {
                    id: object_id_hex.clone(),
                    kind: kind.to_string(),
                    mime_type,
                    size_bytes: i64::try_from(entry.size).unwrap_or(i64::MAX),
                    created_at: now.clone(),
                    updated_at: now.clone(),
                });

                // Reuse cached location_id instead of re-computing BLAKE3 hash.
                let location_id = fid_to_location_id
                    .get(&entry.fid)
                    .cloned()
                    .unwrap_or_else(|| {
                        location_id_for_entry(&self.config.volume_id, &entry.full_path)
                    });
                let extension = entry
                    .full_path
                    .extension()
                    .map(|e| e.to_string_lossy().to_string());

                // Resolve parent_id: look up parent_fid in the fid→LocationId map.
                // Self-referencing entries (parent_fid == fid, e.g. NTFS root) get None
                // to avoid FK violations on self-insert.
                let parent_id = if entry.parent_fid == NO_PARENT_FID
                    || entry.parent_fid == entry.fid
                {
                    None
                } else {
                    fid_to_location_id.get(&entry.parent_fid).cloned()
                };

                location_rows.push(LocationRow {
                    id: location_id,
                    object_id: object_id_hex,
                    volume_id: self.config.volume_id.clone(),
                    path: entry.full_path.to_string_lossy().to_string(),
                    name: entry.name_lossy.clone(),
                    extension,
                    parent_id,
                    is_directory: entry.is_dir,
                    size_bytes: i64::try_from(entry.size).unwrap_or(i64::MAX),
                    allocated_bytes: i64::try_from(entry.allocated_size).unwrap_or(i64::MAX),
                    created_at: now.clone(),
                    modified_at: entry.modified_at.format("%Y-%m-%d %H:%M:%S").to_string(),
                    accessed_at: None,
                    fid: i64::try_from(entry.fid).ok(),
                });
            }

            // Batch upsert: objects first (locations have FK to objects).
            queries::upsert_objects_batch(&self.pool, &object_rows)
                .await
                .map_err(crate::error::PipelineError::Database)?;
            queries::upsert_locations_batch(&self.pool, &location_rows)
                .await
                .map_err(crate::error::PipelineError::Database)?;
        }

        let stats = PipelineBatchComplete {
            total,
            hashed: total_hashed,
            cached: total_cached,
            skipped: total_skipped,
            errors: total_errors,
            directories: total_directories,
            zero_byte: total_zero_byte,
            elapsed: start.elapsed(),
        };

        tracing::info!(
            total = stats.total,
            hashed = stats.hashed,
            cached = stats.cached,
            skipped = stats.skipped,
            elapsed_ms = stats.elapsed.as_millis() as u64,
            "pipeline batch complete"
        );

        Ok(stats)
    }
}

/// Generate a deterministic LocationId from volume_id + path.
///
/// Uses BLAKE3 over raw OS bytes so the same file at the same path always
/// gets the same LocationId — even for non-UTF-8 paths on Linux/macOS.
/// This is critical for idempotent upserts — random UUIDs would create duplicates.
pub fn location_id_for_entry(volume_id: &str, path: &Path) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(volume_id.as_bytes());
    hasher.update(b":");
    hasher.update(path.as_os_str().as_encoded_bytes());
    let hash = hasher.finalize();
    let id = ObjectId::from_bytes(*hash.as_bytes());
    id.to_string()
}

/// Detect MIME type from file extension.
///
/// Returns None for unknown extensions. Covers 50+ common types.
pub fn mime_from_extension(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    let mime = match ext.as_str() {
        // Documents
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odt" => "application/vnd.oasis.opendocument.text",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "csv" => "text/csv",
        // Text
        "txt" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "xml" => "application/xml",
        "json" => "application/json",
        "yaml" | "yml" => "application/yaml",
        "toml" => "application/toml",
        // Images
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "tiff" | "tif" => "image/tiff",
        "avif" => "image/avif",
        "heic" | "heif" => "image/heic",
        // Video
        "mp4" => "video/mp4",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        "mov" => "video/quicktime",
        "wmv" => "video/x-ms-wmv",
        "webm" => "video/webm",
        "flv" => "video/x-flv",
        // Audio
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "aac" => "audio/aac",
        "m4a" => "audio/mp4",
        "wma" => "audio/x-ms-wma",
        // Archives
        "zip" => "application/zip",
        "tar" => "application/x-tar",
        "gz" | "gzip" => "application/gzip",
        "bz2" => "application/x-bzip2",
        "xz" => "application/x-xz",
        "7z" => "application/x-7z-compressed",
        "rar" => "application/vnd.rar",
        "zst" | "zstd" => "application/zstd",
        // Code
        "rs" => "text/x-rust",
        "py" => "text/x-python",
        "js" => "text/javascript",
        "ts" => "text/typescript",
        "go" => "text/x-go",
        "java" => "text/x-java",
        "c" => "text/x-c",
        "cpp" | "cc" | "cxx" => "text/x-c++",
        "h" | "hpp" => "text/x-c-header",
        "rb" => "text/x-ruby",
        "sh" | "bash" => "text/x-shellscript",
        "sql" => "application/sql",
        // Executables
        "exe" => "application/x-msdownload",
        "dll" => "application/x-msdownload",
        "so" => "application/x-sharedlib",
        "dmg" => "application/x-apple-diskimage",
        "msi" => "application/x-msi",
        "deb" => "application/vnd.debian.binary-package",
        "rpm" => "application/x-rpm",
        "appimage" => "application/x-executable",
        // Fonts
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => return None,
    };
    Some(mime.to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use chrono::Utc;
    use hyprdrive_core::db::pool::{create_pool, run_migrations};
    use std::ffi::OsString;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn make_entry(path: PathBuf, size: u64, is_dir: bool) -> IndexEntry {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_FID: AtomicU64 = AtomicU64::new(1000);
        IndexEntry {
            fid: NEXT_FID.fetch_add(1, Ordering::Relaxed),
            parent_fid: NO_PARENT_FID,
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

    async fn setup() -> (SqlitePool, Database, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let pool = create_pool(&db_path).await.unwrap();
        run_migrations(&pool).await.unwrap();
        let cache = Database::create(dir.path().join("cache.redb")).unwrap();
        (pool, cache, dir)
    }

    #[test]
    fn location_id_deterministic() {
        let id1 = location_id_for_entry("vol1", Path::new("/a/b/c.txt"));
        let id2 = location_id_for_entry("vol1", Path::new("/a/b/c.txt"));
        assert_eq!(id1, id2);
    }

    #[test]
    fn location_id_unique_per_path() {
        let id1 = location_id_for_entry("vol1", Path::new("/a/b/c.txt"));
        let id2 = location_id_for_entry("vol1", Path::new("/a/b/d.txt"));
        assert_ne!(id1, id2);
    }

    #[test]
    fn location_id_unique_per_volume() {
        let id1 = location_id_for_entry("vol1", Path::new("/a/b.txt"));
        let id2 = location_id_for_entry("vol2", Path::new("/a/b.txt"));
        assert_ne!(id1, id2);
    }

    #[test]
    fn mime_detection_common_types() {
        assert_eq!(
            mime_from_extension(Path::new("photo.jpg")),
            Some("image/jpeg".to_string())
        );
        assert_eq!(
            mime_from_extension(Path::new("doc.pdf")),
            Some("application/pdf".to_string())
        );
        assert_eq!(
            mime_from_extension(Path::new("code.rs")),
            Some("text/x-rust".to_string())
        );
        assert_eq!(mime_from_extension(Path::new("noext")), None);
        assert_eq!(mime_from_extension(Path::new("file.xyz123")), None);
    }

    #[tokio::test]
    async fn pipeline_single_file() {
        let (pool, cache, dir) = setup().await;
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, b"hello world").unwrap();

        let entries = vec![make_entry(file_path, 11, false)];
        let config = PipelineConfig::new("vol_test".to_string());
        let pipeline = ObjectPipeline::new(config, pool.clone(), cache);

        let stats = pipeline.process_entries(&entries).await.unwrap();
        assert_eq!(stats.total, 1);
        assert_eq!(stats.hashed, 1);
        assert_eq!(stats.skipped, 0);

        // Verify data in DB.
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM objects")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 1);

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM locations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 1);
    }

    #[tokio::test]
    async fn pipeline_directory_entry() {
        let (pool, cache, dir) = setup().await;
        let entries = vec![make_entry(dir.path().join("mydir"), 0, true)];
        let config = PipelineConfig::new("vol_test".to_string());
        let pipeline = ObjectPipeline::new(config, pool.clone(), cache);

        let stats = pipeline.process_entries(&entries).await.unwrap();
        assert_eq!(stats.total, 1);
        assert_eq!(stats.hashed, 0);

        // Directory should still be in DB.
        let (kind,): (String,) = sqlx::query_as("SELECT kind FROM objects LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(kind, "Directory");
    }

    #[tokio::test]
    async fn pipeline_duplicate_files_share_object() {
        let (pool, cache, dir) = setup().await;

        let f1 = dir.path().join("copy1.txt");
        let f2 = dir.path().join("copy2.txt");
        std::fs::write(&f1, b"same content").unwrap();
        std::fs::write(&f2, b"same content").unwrap();

        let entries = vec![make_entry(f1, 12, false), make_entry(f2, 12, false)];
        let config = PipelineConfig::new("vol_test".to_string());
        let pipeline = ObjectPipeline::new(config, pool.clone(), cache);

        let stats = pipeline.process_entries(&entries).await.unwrap();
        assert_eq!(stats.total, 2);

        // 1 object (same content hash), 2 locations.
        let (obj_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM objects")
            .fetch_one(&pool)
            .await
            .unwrap();
        let (loc_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM locations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(obj_count, 1);
        assert_eq!(loc_count, 2);
    }

    #[tokio::test]
    async fn pipeline_idempotent_rerun() {
        let (pool, cache, dir) = setup().await;
        let file_path = dir.path().join("idem.txt");
        std::fs::write(&file_path, b"idempotent").unwrap();

        let entries = vec![make_entry(file_path, 10, false)];
        let config = PipelineConfig::new("vol_test".to_string());
        let pipeline = ObjectPipeline::new(config, pool.clone(), cache);

        pipeline.process_entries(&entries).await.unwrap();
        pipeline.process_entries(&entries).await.unwrap();

        let (obj_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM objects")
            .fetch_one(&pool)
            .await
            .unwrap();
        let (loc_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM locations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(obj_count, 1);
        assert_eq!(loc_count, 1);
    }

    #[tokio::test]
    async fn pipeline_empty_batch() {
        let (pool, cache, _dir) = setup().await;
        let config = PipelineConfig::new("vol_test".to_string());
        let pipeline = ObjectPipeline::new(config, pool, cache);

        let stats = pipeline.process_entries(&[]).await.unwrap();
        assert_eq!(stats.total, 0);
    }

    #[tokio::test]
    async fn pipeline_statistics_accurate() {
        let (pool, cache, dir) = setup().await;

        let f1 = dir.path().join("real.txt");
        std::fs::write(&f1, b"real file").unwrap();

        let entries = vec![
            make_entry(f1, 9, false),
            make_entry(dir.path().join("subdir"), 0, true),
            make_entry(PathBuf::from("/nonexistent/ghost.bin"), 999, false),
        ];
        let config = PipelineConfig::new("vol_test".to_string());
        let pipeline = ObjectPipeline::new(config, pool, cache);

        let stats = pipeline.process_entries(&entries).await.unwrap();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.hashed, 1); // Only real.txt
        assert_eq!(stats.skipped, 1); // ghost.bin
    }
}
