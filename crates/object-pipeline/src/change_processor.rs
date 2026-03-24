//! Change processor — dispatches batched FsChange events to the right handler.
//!
//! Created entries are enriched and fed through the pipeline.
//! Deleted entries remove locations and clean up orphaned objects.
//! Moved entries update the location path via DELETE + INSERT.
//! Modified entries re-hash and upsert updated objects/locations.

use crate::error::PipelineResult;
use crate::pipeline::{location_id_for_entry, ObjectPipeline, PipelineConfig, NO_PARENT_FID};
use chrono::Utc;
use hyprdrive_core::db::queries;
use hyprdrive_fs_indexer::{FsChange, IndexEntry};
use redb::Database;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// Stats from processing a batch of changes.
#[derive(Debug, Default)]
pub struct ChangeStats {
    pub created: usize,
    pub deleted: usize,
    pub moved: usize,
    pub modified: usize,
    pub errors: usize,
    pub rescan_needed: bool,
}

/// Processes batched FsChange events — dispatches to pipeline, delete, or relocate.
pub struct ChangeProcessor {
    volume_id: String,
    pool: SqlitePool,
    cache: Arc<Database>,
    /// In-memory fid→path map for fast path resolution.
    fid_map: Arc<RwLock<HashMap<u64, PathBuf>>>,
}

impl ChangeProcessor {
    pub fn new(volume_id: String, pool: SqlitePool, cache: Arc<Database>) -> Self {
        Self {
            volume_id,
            pool,
            cache,
            fid_map: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Populate fid_map from initial scan results (call once after scan).
    #[allow(clippy::expect_used)] // Panic on poisoned lock is intentional.
    pub fn seed_fid_map(&self, entries: &[IndexEntry]) {
        let mut map = self.fid_map.write().expect("fid_map write lock");
        for entry in entries {
            map.insert(entry.fid, entry.full_path.clone());
        }
    }

    /// Resolve parent_fid + name → full path using fid_map.
    #[allow(clippy::expect_used)] // Panic on poisoned lock is intentional.
    fn resolve_path(&self, parent_fid: u64, name: &OsStr) -> Option<PathBuf> {
        let map = self.fid_map.read().expect("fid_map read lock");
        map.get(&parent_fid).map(|parent| parent.join(name))
    }

    /// Get the path for a given fid.
    #[allow(clippy::expect_used)] // Panic on poisoned lock is intentional.
    fn get_path(&self, fid: u64) -> Option<PathBuf> {
        let map = self.fid_map.read().expect("fid_map read lock");
        map.get(&fid).cloned()
    }

    /// Process a batch of FsChange events.
    #[allow(clippy::expect_used)] // Panic on poisoned lock is intentional.
    pub async fn process_changes(&self, changes: Vec<FsChange>) -> PipelineResult<ChangeStats> {
        let mut stats = ChangeStats::default();

        // Partition changes into buckets.
        let mut creates: Vec<IndexEntry> = Vec::new();
        let mut deletes: Vec<(u64, Option<PathBuf>)> = Vec::new();
        let mut moves: Vec<(u64, u64, std::ffi::OsString)> = Vec::new();
        let mut modifies: Vec<(u64, u64)> = Vec::new();

        for change in changes {
            match change {
                FsChange::Created(entry) => creates.push(entry),
                FsChange::Deleted { fid, path } => deletes.push((fid, path)),
                FsChange::Moved {
                    fid,
                    new_parent_fid,
                    new_name,
                } => moves.push((fid, new_parent_fid, new_name)),
                FsChange::Modified { fid, new_size } => modifies.push((fid, new_size)),
                FsChange::FullRescanNeeded { .. } => {
                    stats.rescan_needed = true;
                }
            }
        }

        // ── Process Creates ──
        if !creates.is_empty() {
            let config = PipelineConfig::new(self.volume_id.clone());
            let pipeline =
                ObjectPipeline::new_shared(config, self.pool.clone(), Arc::clone(&self.cache));

            // Enrich sparse entries: resolve full_path from fid_map.
            let mut enriched: Vec<IndexEntry> = Vec::new();
            for mut entry in creates {
                // If full_path is empty (sparse USN entry), resolve it.
                if entry.full_path.as_os_str().is_empty() {
                    if let Some(path) = self.resolve_path(entry.parent_fid, &entry.name) {
                        entry.full_path = path.clone();
                        // Stat the file for size/timestamps if it exists.
                        if let Ok(meta) = std::fs::metadata(&path) {
                            entry.size = meta.len();
                            entry.is_dir = meta.is_dir();
                            if let Ok(modified) = meta.modified() {
                                entry.modified_at = chrono::DateTime::<Utc>::from(modified);
                            }
                        }
                    } else {
                        stats.errors += 1;
                        continue;
                    }
                }

                // Update fid_map with new entry.
                {
                    let mut map = self.fid_map.write().expect("fid_map write lock");
                    map.insert(entry.fid, entry.full_path.clone());
                }

                enriched.push(entry);
            }

            if !enriched.is_empty() {
                match pipeline.process_entries(&enriched).await {
                    Ok(_) => stats.created += enriched.len(),
                    Err(e) => {
                        tracing::error!(error = %e, "failed to process created entries");
                        stats.errors += enriched.len();
                    }
                }
            }
        }

        // ── Process Deletes ──
        let mut orphan_candidates: Vec<String> = Vec::new();
        for (fid, path) in &deletes {
            // Try fid-based deletion first (works on Windows where fid = file reference number).
            let mut deleted_object_id: Option<String> = None;

            if let Ok(fid_i64) = i64::try_from(*fid) {
                match queries::delete_location_by_fid(&self.pool, &self.volume_id, fid_i64).await {
                    Ok(Some(object_id)) => {
                        deleted_object_id = Some(object_id);
                    }
                    Ok(None) => {} // Not found by fid — try path fallback below
                    Err(e) => {
                        tracing::error!(fid, error = %e, "failed to delete location by fid");
                        stats.errors += 1;
                        continue;
                    }
                }
            }

            // Fallback: path-based deletion (Linux where fid is a hash, not a real inode).
            if deleted_object_id.is_none() {
                if let Some(ref p) = path {
                    let path_str = p.to_string_lossy();
                    match queries::delete_location_by_path(
                        &self.pool,
                        &self.volume_id,
                        &path_str,
                    )
                    .await
                    {
                        Ok(Some(object_id)) => {
                            deleted_object_id = Some(object_id);
                        }
                        Ok(None) => {
                            tracing::debug!(fid, path = %path_str, "delete: location not found by fid or path");
                        }
                        Err(e) => {
                            tracing::error!(fid, path = %path_str, error = %e, "failed to delete location by path");
                            stats.errors += 1;
                            continue;
                        }
                    }
                } else {
                    tracing::debug!(fid, "delete: location not found by fid (no path fallback)");
                }
            }

            if let Some(object_id) = deleted_object_id {
                stats.deleted += 1;
                orphan_candidates.push(object_id);
                // Remove from fid_map.
                let mut map = self.fid_map.write().expect("fid_map write lock");
                map.remove(fid);
            }
        }

        // Clean up orphaned objects.
        if !orphan_candidates.is_empty() {
            let orphan_count = orphan_candidates.len();
            if let Err(e) = queries::delete_orphan_objects(&self.pool, &orphan_candidates).await {
                tracing::error!(error = %e, "failed to clean up orphan objects");
                stats.errors += orphan_count;
            }
        }

        // ── Process Moves ──
        for (fid, new_parent_fid, new_name) in &moves {
            let new_path = match self.resolve_path(*new_parent_fid, new_name) {
                Some(p) => p,
                None => {
                    tracing::debug!(fid, "move: cannot resolve new parent path");
                    stats.errors += 1;
                    continue;
                }
            };

            let new_name_str = new_name.to_string_lossy().to_string();
            let new_extension = new_path
                .extension()
                .map(|e| e.to_string_lossy().to_string());
            let new_location_id = location_id_for_entry(&self.volume_id, &new_path);

            // Derive new_parent_id from the parent directory path.
            let new_parent_dir = new_path
                .parent()
                .map(|p| location_id_for_entry(&self.volume_id, p));

            let fid_i64 = match i64::try_from(*fid) {
                Ok(v) => v,
                Err(_) => {
                    tracing::warn!(fid, "move: fid exceeds i64::MAX, skipping");
                    stats.errors += 1;
                    continue;
                }
            };
            match queries::relocate_location(
                &self.pool,
                &self.volume_id,
                fid_i64,
                &new_location_id,
                &new_path.to_string_lossy(),
                &new_name_str,
                new_extension.as_deref(),
                new_parent_dir.as_deref(),
            )
            .await
            {
                Ok(true) => {
                    stats.moved += 1;
                    // Update fid_map.
                    let mut map = self.fid_map.write().expect("fid_map write lock");
                    map.insert(*fid, new_path);
                }
                Ok(false) => {
                    tracing::debug!(fid, "move: location not found");
                }
                Err(e) => {
                    tracing::error!(fid, error = %e, "failed to relocate location");
                    stats.errors += 1;
                }
            }
        }

        // ── Process Modifies ──
        if !modifies.is_empty() {
            let config = PipelineConfig::new(self.volume_id.clone());
            let pipeline =
                ObjectPipeline::new_shared(config, self.pool.clone(), Arc::clone(&self.cache));

            for (fid, new_size) in &modifies {
                let path = match self.get_path(*fid) {
                    Some(p) => p,
                    None => {
                        tracing::debug!(fid, "modify: path not found in fid_map");
                        stats.errors += 1;
                        continue;
                    }
                };

                // Skip entries with no file name (e.g. volume roots).
                let file_name = match path.file_name() {
                    Some(n) if !n.is_empty() => n,
                    _ => {
                        tracing::debug!(fid, path = %path.display(), "modify: no file name");
                        stats.errors += 1;
                        continue;
                    }
                };

                // Build a synthetic IndexEntry for re-processing.
                let name = file_name.to_string_lossy().to_string();
                let is_dir = path.is_dir();
                let modified_at = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .map(chrono::DateTime::<Utc>::from)
                    .unwrap_or_else(|_| Utc::now());

                let entry = IndexEntry {
                    fid: *fid,
                    parent_fid: NO_PARENT_FID, // not needed for re-processing
                    name: file_name.to_owned(),
                    name_lossy: name,
                    full_path: path.clone(),
                    size: *new_size,
                    allocated_size: *new_size, // approximate
                    is_dir,
                    modified_at,
                    attributes: 0,
                };

                match pipeline.process_entries(&[entry]).await {
                    Ok(_) => stats.modified += 1,
                    Err(e) => {
                        tracing::error!(fid, error = %e, "failed to process modified entry");
                        stats.errors += 1;
                    }
                }
            }
        }

        Ok(stats)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_change_stats_defaults() {
        let stats = ChangeStats::default();
        assert_eq!(stats.created, 0);
        assert_eq!(stats.deleted, 0);
        assert_eq!(stats.moved, 0);
        assert_eq!(stats.modified, 0);
        assert_eq!(stats.errors, 0);
        assert!(!stats.rescan_needed);
    }

    #[test]
    fn test_seed_fid_map() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let cache_path = dir.path().join("cache.redb");
        let cache = Arc::new(Database::create(&cache_path).expect("redb"));
        let pool_rt = tokio::runtime::Runtime::new().unwrap();
        let pool = pool_rt.block_on(async {
            let db_path = dir.path().join("test.db");
            let pool = hyprdrive_core::db::pool::create_pool(&db_path)
                .await
                .expect("pool");
            hyprdrive_core::db::pool::run_migrations(&pool)
                .await
                .expect("migrations");
            pool
        });

        let processor = ChangeProcessor::new("C".to_string(), pool, cache);
        let entries = vec![IndexEntry {
            fid: 42,
            parent_fid: 0,
            name: "test.txt".into(),
            name_lossy: "test.txt".to_string(),
            full_path: PathBuf::from("C:\\test.txt"),
            size: 100,
            allocated_size: 4096,
            is_dir: false,
            modified_at: Utc::now(),
            attributes: 0,
        }];

        processor.seed_fid_map(&entries);

        let path = processor.get_path(42);
        assert_eq!(path, Some(PathBuf::from("C:\\test.txt")));
    }

    #[test]
    fn test_resolve_path() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let cache_path = dir.path().join("cache.redb");
        let cache = Arc::new(Database::create(&cache_path).expect("redb"));
        let pool_rt = tokio::runtime::Runtime::new().unwrap();
        let pool = pool_rt.block_on(async {
            let db_path = dir.path().join("test.db");
            let pool = hyprdrive_core::db::pool::create_pool(&db_path)
                .await
                .expect("pool");
            hyprdrive_core::db::pool::run_migrations(&pool)
                .await
                .expect("migrations");
            pool
        });

        let processor = ChangeProcessor::new("C".to_string(), pool, cache);

        // Seed a parent directory.
        let parent = IndexEntry {
            fid: 10,
            parent_fid: 0,
            name: "docs".into(),
            name_lossy: "docs".to_string(),
            full_path: PathBuf::from("C:\\docs"),
            size: 0,
            allocated_size: 0,
            is_dir: true,
            modified_at: Utc::now(),
            attributes: 0,
        };
        processor.seed_fid_map(&[parent]);

        let resolved = processor.resolve_path(10, OsStr::new("readme.md"));
        let expected = PathBuf::from("C:\\docs").join("readme.md");
        assert_eq!(resolved, Some(expected));

        // Missing parent returns None.
        let missing = processor.resolve_path(999, OsStr::new("nope.txt"));
        assert!(missing.is_none());
    }
}
