//! File hashing wrapper over dedup-engine's BLAKE3 hasher.
//!
//! Converts raw `[u8; 32]` hashes to domain `ObjectId` types, integrates
//! with the redb inode cache for skip-on-hit, and provides rayon-parallel
//! batch hashing.

use crate::error::{PipelineError, PipelineResult};
use hyprdrive_core::db::cache::inode;
use hyprdrive_core::domain::id::ObjectId;
use hyprdrive_fs_indexer::types::IndexEntry;
use rayon::prelude::*;
use redb::Database;
use std::path::Path;

/// Hash a file's content and return its content-addressed `ObjectId`.
///
/// Delegates to dedup-engine's progressive BLAKE3 hasher:
/// - Streaming 64KB chunks for files <= 512MB
/// - Memory-mapped I/O for files > 512MB
/// - Empty files hash to `BLAKE3("")`
#[tracing::instrument(skip_all, fields(path = %path.display()))]
pub fn hash_file(path: &Path) -> PipelineResult<ObjectId> {
    let hash_bytes = hyprdrive_dedup_engine::hasher::full_hash(path)
        .map_err(|e| PipelineError::Hash(e.to_string()))?;
    Ok(ObjectId::from_bytes(hash_bytes))
}

/// Result of hashing a single entry.
#[derive(Debug)]
pub struct HashResult {
    /// Index into the entries slice passed to [`hash_entries_batch`].
    ///
    /// This is a chunk-local index: when the pipeline processes entries in
    /// batches, each batch is a standalone slice and `index` refers to a
    /// position within that slice.
    pub index: usize,
    /// The computed ObjectId (content hash or synthetic).
    pub object_id: ObjectId,
    /// Whether this was a cache hit (no file I/O needed).
    pub cached: bool,
    /// Whether this ObjectId was derived synthetically (e.g. directory entries)
    /// rather than by hashing file content or hitting the inode cache.
    pub synthetic: bool,
}

/// Result of hashing a batch of entries.
#[derive(Debug)]
pub struct BatchHashResult {
    /// Successfully hashed entries.
    pub results: Vec<HashResult>,
    /// Number of cache hits.
    pub cache_hits: usize,
    /// Number of files actually hashed (cache misses).
    pub hashed: usize,
    /// Number of entries skipped due to errors.
    pub skipped: usize,
    /// Indices of skipped entries and their error messages.
    pub errors: Vec<(usize, String)>,
}

/// Hash a batch of `IndexEntry` values, checking the inode cache first.
///
/// 1. Build cache keys for all entries
/// 2. Batch-lookup in redb (single read transaction)
/// 3. Hash cache misses in parallel via rayon
/// 4. Batch-insert new cache entries (single write transaction)
///
/// Directories get a synthetic `ObjectId` derived from their path
/// and are never hashed from disk.
#[tracing::instrument(skip_all, fields(count = entries.len(), volume_id))]
pub fn hash_entries_batch(
    entries: &[IndexEntry],
    cache: &Database,
    volume_id: &str,
) -> BatchHashResult {
    if entries.is_empty() {
        return BatchHashResult {
            results: Vec::new(),
            cache_hits: 0,
            hashed: 0,
            skipped: 0,
            errors: Vec::new(),
        };
    }

    // Build cache keys for all entries.
    let cache_keys: Vec<String> = entries
        .iter()
        .map(|e| inode::cache_key_v2(volume_id, e.fid, e.modified_at.timestamp(), e.size))
        .collect();

    // Batch lookup in redb.
    let key_refs: Vec<&str> = cache_keys.iter().map(|k| k.as_str()).collect();
    let cached_values = inode::get_batch(cache, &key_refs).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "inode cache batch read failed, falling back to full hash");
        vec![None; entries.len()]
    });

    // Classify entries: directory, cache hit, or needs hashing.
    let mut results = Vec::with_capacity(entries.len());
    let mut needs_hashing: Vec<usize> = Vec::new();
    let mut cache_hits = 0usize;

    for (i, entry) in entries.iter().enumerate() {
        if entry.is_dir {
            // Directories get a synthetic ObjectId — no file I/O.
            // Uses raw OS bytes to avoid lossy UTF-8 conversion collisions.
            let mut hasher = blake3::Hasher::new();
            hasher.update(b"dir:");
            hasher.update(entry.full_path.as_os_str().as_encoded_bytes());
            let hash = hasher.finalize();
            let object_id = ObjectId::from_bytes(*hash.as_bytes());
            results.push(Ok(HashResult {
                index: i,
                object_id,
                cached: false,
                synthetic: true,
            }));
        } else if let Some(ref cached_hex) = cached_values[i] {
            // Cache hit — parse the stored ObjectId hex string.
            match cached_hex.parse::<ObjectId>() {
                Ok(object_id) => {
                    cache_hits += 1;
                    results.push(Ok(HashResult {
                        index: i,
                        object_id,
                        cached: true,
                        synthetic: false,
                    }));
                }
                Err(_) => {
                    // Corrupt cache entry — re-hash.
                    needs_hashing.push(i);
                    results.push(Err(i)); // Placeholder.
                }
            }
        } else {
            needs_hashing.push(i);
            results.push(Err(i)); // Placeholder.
        }
    }

    // Hash cache misses in parallel via rayon.
    let hash_results: Vec<(usize, Result<ObjectId, String>)> = needs_hashing
        .par_iter()
        .map(|&i| {
            let entry = &entries[i];
            if entry.size == 0 {
                // Zero-byte file — deterministic hash without file I/O.
                return (i, Ok(ObjectId::from_blake3(&[])));
            }
            match hash_file(&entry.full_path) {
                Ok(id) => (i, Ok(id)),
                Err(e) => {
                    // Check if it's a permission error or missing file.
                    let err_str = e.to_string();
                    tracing::warn!(
                        path = %entry.full_path.display(),
                        error = %err_str,
                        "skipping file"
                    );
                    (i, Err(err_str))
                }
            }
        })
        .collect();

    // Collect results and new cache entries.
    let mut hashed = 0usize;
    let mut skipped = 0usize;
    let mut errors = Vec::new();
    let mut new_cache_entries: Vec<(String, String)> = Vec::new();

    for (i, result) in hash_results {
        match result {
            Ok(object_id) => {
                hashed += 1;
                new_cache_entries.push((cache_keys[i].clone(), object_id.to_string()));
                debug_assert!(results[i].is_err(), "slot {i} should be a placeholder");
                results[i] = Ok(HashResult {
                    index: i,
                    object_id,
                    cached: false,
                    synthetic: false,
                });
            }
            Err(err_msg) => {
                skipped += 1;
                errors.push((i, err_msg));
                // Remove placeholder — filter out Err entries later.
            }
        }
    }

    // Batch-insert new cache entries (single write transaction).
    if !new_cache_entries.is_empty() {
        let refs: Vec<(&str, &str)> = new_cache_entries
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        if let Err(e) = inode::insert_batch(cache, &refs) {
            tracing::warn!(error = %e, "failed to write inode cache batch");
        }
    }

    // Flatten results, keeping only Ok entries.
    let final_results: Vec<HashResult> = results.into_iter().filter_map(|r| r.ok()).collect();

    BatchHashResult {
        results: final_results,
        cache_hits,
        hashed,
        skipped,
        errors,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::ffi::OsString;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::{NamedTempFile, TempDir};

    fn write_temp(content: &[u8]) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content).unwrap();
        f.flush().unwrap();
        f
    }

    fn test_cache() -> (Database, TempDir) {
        let dir = TempDir::new().unwrap();
        let db = Database::create(dir.path().join("cache.redb")).unwrap();
        (db, dir)
    }

    fn make_entry(path: PathBuf, size: u64, is_dir: bool) -> IndexEntry {
        IndexEntry {
            fid: 1000 + size,
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

    // ═══ hash_file tests ═══

    #[test]
    fn hash_file_deterministic() {
        let f = write_temp(b"hello world");
        let h1 = hash_file(f.path()).unwrap();
        let h2 = hash_file(f.path()).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_file_content_addressed() {
        let f1 = write_temp(b"identical");
        let f2 = write_temp(b"identical");
        assert_eq!(hash_file(f1.path()).unwrap(), hash_file(f2.path()).unwrap());
    }

    #[test]
    fn hash_file_different_content() {
        let f1 = write_temp(b"content A");
        let f2 = write_temp(b"content B");
        assert_ne!(hash_file(f1.path()).unwrap(), hash_file(f2.path()).unwrap());
    }

    #[test]
    fn hash_file_empty() {
        let f = write_temp(b"");
        let id = hash_file(f.path()).unwrap();
        assert_eq!(id, ObjectId::from_blake3(&[]));
    }

    #[test]
    fn hash_file_nonexistent() {
        let result = hash_file(Path::new("/nonexistent/file.txt"));
        assert!(result.is_err());
    }

    #[test]
    fn hash_file_matches_dedup_engine() {
        let content = b"test content for matching";
        let f = write_temp(content);
        let pipeline_id = hash_file(f.path()).unwrap();
        let raw_hash = hyprdrive_dedup_engine::hasher::full_hash(f.path()).unwrap();
        assert_eq!(pipeline_id, ObjectId::from_bytes(raw_hash));
    }

    // ═══ hash_entries_batch tests ═══

    #[test]
    fn batch_empty() {
        let (cache, _dir) = test_cache();
        let result = hash_entries_batch(&[], &cache, "vol1");
        assert!(result.results.is_empty());
        assert_eq!(result.cache_hits, 0);
        assert_eq!(result.hashed, 0);
    }

    #[test]
    fn batch_directory_skips_io() {
        let (cache, _dir) = test_cache();
        let entries = vec![make_entry(PathBuf::from("/test/mydir"), 0, true)];
        let result = hash_entries_batch(&entries, &cache, "vol1");
        assert_eq!(result.results.len(), 1);
        assert!(result.results[0].synthetic);
        assert!(!result.results[0].cached);
        assert_eq!(result.hashed, 0);
    }

    #[test]
    fn batch_cache_miss_then_hit() {
        let (cache, _cdir) = test_cache();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, b"hello").unwrap();

        let entries = vec![make_entry(path.clone(), 5, false)];

        // First call: cache miss, hashes the file.
        let r1 = hash_entries_batch(&entries, &cache, "vol1");
        assert_eq!(r1.hashed, 1);
        assert_eq!(r1.cache_hits, 0);
        let first_id = r1.results[0].object_id;

        // Second call: cache hit, same ObjectId.
        let r2 = hash_entries_batch(&entries, &cache, "vol1");
        assert_eq!(r2.cache_hits, 1);
        assert_eq!(r2.hashed, 0);
        assert_eq!(r2.results[0].object_id, first_id);
    }

    #[test]
    fn batch_size_change_invalidates_cache() {
        let (cache, _cdir) = test_cache();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("growing.txt");
        std::fs::write(&path, b"small").unwrap();

        let entry1 = make_entry(path.clone(), 5, false);
        let r1 = hash_entries_batch(&[entry1], &cache, "vol1");
        let id1 = r1.results[0].object_id;

        // "Grow" the file and make a new entry with different size.
        std::fs::write(&path, b"much bigger content now").unwrap();
        let entry2 = make_entry(path.clone(), 22, false);
        let r2 = hash_entries_batch(&[entry2], &cache, "vol1");

        // Should be a cache miss because size changed.
        assert_eq!(r2.hashed, 1);
        assert_ne!(r2.results[0].object_id, id1);
    }

    #[test]
    fn batch_zero_byte_no_io() {
        let (cache, _cdir) = test_cache();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("empty.txt");
        std::fs::write(&path, b"").unwrap();

        let entries = vec![make_entry(path, 0, false)];
        let result = hash_entries_batch(&entries, &cache, "vol1");
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].object_id, ObjectId::from_blake3(&[]));
    }

    #[test]
    fn batch_missing_file_skipped() {
        let (cache, _cdir) = test_cache();
        let entries = vec![make_entry(
            PathBuf::from("/nonexistent/file.txt"),
            100,
            false,
        )];
        let result = hash_entries_batch(&entries, &cache, "vol1");
        assert_eq!(result.skipped, 1);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn batch_mixed_files_and_dirs() {
        let (cache, _cdir) = test_cache();
        let dir = TempDir::new().unwrap();

        let file_path = dir.path().join("file.txt");
        std::fs::write(&file_path, b"content").unwrap();

        let entries = vec![
            make_entry(dir.path().to_path_buf(), 0, true),
            make_entry(file_path, 7, false),
        ];
        let result = hash_entries_batch(&entries, &cache, "vol1");
        assert_eq!(result.results.len(), 2);
        assert_eq!(result.hashed, 1); // Only the file.
    }
}
