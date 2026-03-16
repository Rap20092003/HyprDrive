//! Size enrichment via batched lstat() calls.
//!
//! Fills `size`, `allocated_size`, and `modified_at` fields on [`IndexEntry`]
//! values using `lstat()` (symlink_metadata) — the Linux equivalent of
//! Windows `GetFileInformationByHandleEx(FileStandardInfo)`.

use crate::error::FsIndexerResult;
use crate::types::IndexEntry;
use chrono::DateTime;
use std::os::unix::fs::MetadataExt;

/// Statistics from the enrichment pass.
#[derive(Debug, Clone)]
pub struct EnrichStats {
    /// Number of entries successfully enriched.
    pub enriched: usize,
    /// Number of entries skipped (permission denied, deleted, etc.).
    pub skipped: usize,
}

/// Batch size for progress logging.
const BATCH_SIZE: usize = 1000;

/// Enrich [`IndexEntry`] slice with sizes from `lstat()` calls.
///
/// Fills `size`, `allocated_size`, and `modified_at` for each file entry.
/// Directory entries are skipped (they don't have meaningful file size).
/// Entries that fail `lstat()` get `size = 0` with a warning log.
///
/// Uses batching (1000 entries at a time) for progress logging.
#[tracing::instrument(skip(entries), fields(entry_count = entries.len()))]
pub fn enrich_sizes(entries: &mut [IndexEntry]) -> FsIndexerResult<EnrichStats> {
    let mut enriched = 0_usize;
    let mut skipped = 0_usize;
    let total = entries.len();

    for (i, entry) in entries.iter_mut().enumerate() {
        // Skip directories — they don't have meaningful file size
        if entry.is_dir {
            continue;
        }

        match std::fs::symlink_metadata(&entry.full_path) {
            Ok(metadata) => {
                entry.size = metadata.len();
                // st_blocks is in 512-byte units
                entry.allocated_size = metadata.blocks() * 512;
                // Update modified_at from metadata
                if let Ok(modified) = metadata.modified() {
                    if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                        if let Some(dt) = DateTime::from_timestamp(
                            duration.as_secs() as i64,
                            duration.subsec_nanos(),
                        ) {
                            entry.modified_at = dt;
                        }
                    }
                }
                enriched += 1;
            }
            Err(e) => {
                tracing::warn!(
                    path = %entry.full_path.display(),
                    error = %e,
                    "lstat failed, skipping"
                );
                skipped += 1;
            }
        }

        // Progress logging every BATCH_SIZE entries
        if (i + 1) % BATCH_SIZE == 0 {
            tracing::debug!(
                progress = i + 1,
                total = total,
                enriched = enriched,
                skipped = skipped,
                "enrichment batch complete"
            );
        }
    }

    tracing::info!(enriched = enriched, skipped = skipped, "enrichment complete");

    Ok(EnrichStats { enriched, skipped })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::ffi::OsString;
    use std::path::PathBuf;

    /// Helper to create a test IndexEntry.
    fn make_entry(path: PathBuf, is_dir: bool) -> IndexEntry {
        IndexEntry {
            fid: 1,
            parent_fid: 0,
            name: OsString::from(
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
            ),
            name_lossy: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            full_path: path,
            size: 0,
            allocated_size: 0,
            is_dir,
            modified_at: Utc::now(),
            attributes: 0,
        }
    }

    #[test]
    fn enrich_sets_size() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello world").expect("write");

        let mut entries = vec![make_entry(file_path, false)];
        let stats = enrich_sizes(&mut entries).expect("enrich should succeed");

        assert_eq!(stats.enriched, 1);
        assert_eq!(stats.skipped, 0);
        assert_eq!(entries[0].size, 11); // "hello world" = 11 bytes
    }

    #[test]
    fn enrich_sets_allocated_size() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello").expect("write");

        let mut entries = vec![make_entry(file_path, false)];
        enrich_sizes(&mut entries).expect("enrich");

        // allocated_size should be >= size (block-aligned)
        assert!(
            entries[0].allocated_size >= entries[0].size,
            "allocated_size {} should be >= size {}",
            entries[0].allocated_size,
            entries[0].size
        );
    }

    #[test]
    fn enrich_skips_deleted_file() {
        let path = PathBuf::from("/tmp/nonexistent_file_12345.txt");
        let mut entries = vec![make_entry(path, false)];
        let stats = enrich_sizes(&mut entries).expect("enrich should not panic");

        assert_eq!(stats.enriched, 0);
        assert_eq!(stats.skipped, 1);
        assert_eq!(entries[0].size, 0);
    }

    #[test]
    fn enrich_skips_directories() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        let mut entries = vec![make_entry(dir.path().to_path_buf(), true)];
        let stats = enrich_sizes(&mut entries).expect("enrich");

        // Directories are skipped entirely
        assert_eq!(stats.enriched, 0);
        assert_eq!(stats.skipped, 0);
        assert_eq!(entries[0].size, 0);
    }
}
