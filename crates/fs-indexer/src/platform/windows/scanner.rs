//! Combined full scan — MFT topology + size enrichment.
//!
//! Composes the two-phase approach into a single `full_scan` function.

use crate::error::{FsIndexerError, FsIndexerResult};
use crate::platform::windows::{detect, enrich, mft, usn};
use crate::types::{FilesystemKind, IndexEntry, ScanResult};
use chrono::Utc;
use std::ffi::OsString;
use std::path::Path;

/// Minimum synthetic FID for jwalk entries (avoids collision with NTFS FRNs).
const MIN_SYNTHETIC_FID: u64 = 1_000_000_000;

/// Perform a full scan of an NTFS volume.
///
/// 1. Detects filesystem type (must be NTFS).
/// 2. Enumerates MFT topology (FRN tree, no sizes).
/// 3. Builds parent chain → full paths.
/// 4. Enriches sizes via `GetFileInformationByHandleEx`.
/// 5. Returns all entries with populated fields.
///
/// For non-NTFS volumes, use [`fallback_scan`] instead.
#[tracing::instrument(fields(volume = %volume.display()), skip(volume))]
pub fn full_scan(volume: &Path) -> FsIndexerResult<ScanResult> {
    let fs_kind = detect::detect_filesystem(volume)?;
    full_scan_inner(volume, fs_kind)
}

fn full_scan_inner(volume: &Path, fs_kind: FilesystemKind) -> FsIndexerResult<ScanResult> {
    let start = std::time::Instant::now();

    // Phase 0: Verify NTFS
    if fs_kind != FilesystemKind::Ntfs {
        return Err(FsIndexerError::UnsupportedFs { kind: fs_kind });
    }

    // Phase 1: MFT topology
    let topo_entries = mft::mft_enumerate_topology(volume)?;
    let topo_count = topo_entries.len();
    tracing::info!(entries = topo_count, "topology pass complete");

    // Build parent map for path reconstruction
    let mut parent_map = mft::build_parent_map(&topo_entries);

    // Dynamically detect the NTFS root FRN. usn-journal-rs returns full
    // 64-bit file references (record number + sequence number in upper bits),
    // so the root is NOT always 5 — it could be e.g. 0x0001000000000005.
    // Strategy: find the most-referenced parent_fid that has no entry in
    // parent_map (i.e. it was filtered out by MIN_USER_FRN).
    {
        let mut missing_parents: std::collections::HashMap<u64, usize> =
            std::collections::HashMap::new();
        for entry in &topo_entries {
            if !parent_map.contains_key(&entry.parent_fid) {
                *missing_parents.entry(entry.parent_fid).or_default() += 1;
            }
        }
        if let Some((&root_fid, &count)) = missing_parents.iter().max_by_key(|(_, c)| *c) {
            tracing::info!(
                root_fid,
                root_fid_hex = format!("0x{:016X}", root_fid),
                references = count,
                "detected NTFS root FRN dynamically"
            );
            parent_map.insert(root_fid, (root_fid, OsString::new()));
        } else {
            tracing::warn!("no missing parent FRN detected — root may already be in map");
        }
    }

    // Phase 2: Convert to IndexEntry with paths, then enrich sizes
    // Use memoized path reconstruction — much faster for large trees because
    // sibling files in the same directory share cached parent path segments.
    let fids: Vec<u64> = topo_entries.iter().map(|t| t.fid).collect();
    let path_cache = mft::reconstruct_paths_cached(&fids, &parent_map, volume);

    let mut entries: Vec<IndexEntry> = Vec::with_capacity(topo_entries.len());
    let mut orphan_count = 0u64;
    for topo in topo_entries {
        let full_path = match path_cache.get(&topo.fid) {
            Some(p) => p.clone(),
            None => {
                orphan_count += 1;
                continue;
            }
        };
        let name_lossy = topo.name.to_string_lossy().to_string();
        entries.push(IndexEntry {
            fid: topo.fid,
            parent_fid: topo.parent_fid,
            name: topo.name,
            name_lossy,
            full_path,
            size: 0,           // filled by enrichment
            allocated_size: 0, // filled by enrichment
            is_dir: topo.is_dir,
            modified_at: Utc::now(), // updated during enrichment if possible
            attributes: topo.attributes,
        });
    }
    if orphan_count > 0 {
        tracing::info!(orphan_count, "skipped entries with unresolvable paths");
    }

    // Phase 3: Size enrichment
    let enrich_stats = enrich::enrich_sizes(&mut entries)?;
    tracing::debug!(?enrich_stats, "enrichment complete");

    let duration = start.elapsed();
    tracing::info!(
        entries = entries.len(),
        duration_ms = duration.as_millis(),
        "full scan complete"
    );

    // Read USN journal cursor for subsequent delta queries
    let cursor = match usn::read_cursor(volume) {
        Ok(c) => Some(c),
        Err(e) => {
            tracing::warn!(error = %e, "could not read USN cursor — delta queries unavailable");
            None
        }
    };

    Ok(ScanResult {
        entries,
        cursor,
        linux_cursor: None,
    })
}

/// Fallback scan using `jwalk` for non-NTFS volumes or non-admin runs.
///
/// Slower than MFT (~3-5s vs ~1.5s for 100k files) but works without
/// elevated privileges and on FAT32/exFAT volumes.
#[tracing::instrument(fields(volume = %volume.display()), skip(volume))]
pub fn fallback_scan(volume: &Path) -> FsIndexerResult<ScanResult> {
    let start = std::time::Instant::now();
    tracing::info!("starting jwalk fallback scan");

    let mut entries = Vec::new();
    let mut fid_counter: u64 = MIN_SYNTHETIC_FID;
    // Map directory paths to their synthetic fids for accurate parent lookups
    let mut dir_fid_map = std::collections::HashMap::<std::path::PathBuf, u64>::new();

    for dir_entry_result in jwalk::WalkDir::new(volume)
        .skip_hidden(false)
        .follow_links(false)
    {
        let dir_entry = match dir_entry_result {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(error = %e, "jwalk entry error, skipping");
                continue;
            }
        };

        let metadata = match dir_entry.metadata() {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(
                    path = %dir_entry.path().display(),
                    error = %e,
                    "metadata access failed, skipping"
                );
                continue;
            }
        };

        let name = dir_entry.file_name().to_os_string();
        let name_lossy = name.to_string_lossy().to_string();
        let full_path = dir_entry.path();
        let size = metadata.len();
        // FAT32 doesn't expose allocation size — estimate from 4KB clusters
        let allocated_size = if size == 0 {
            0
        } else {
            size.div_ceil(4096) * 4096
        };
        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|t| {
                let duration = t.duration_since(std::time::UNIX_EPOCH).ok()?;
                chrono::DateTime::from_timestamp(duration.as_secs() as i64, duration.subsec_nanos())
            })
            .unwrap_or_else(Utc::now);

        let fid = fid_counter;
        fid_counter += 1;

        // Look up parent fid from directory path map
        let parent_fid = if dir_entry.depth() == 0 {
            fid // root entry: parent is self
        } else {
            full_path
                .parent()
                .and_then(|p| dir_fid_map.get(p).copied())
                .unwrap_or(MIN_SYNTHETIC_FID) // fallback to volume root fid
        };

        // Register directories so children can find their parent
        if metadata.is_dir() {
            dir_fid_map.insert(full_path.clone(), fid);
        }

        entries.push(IndexEntry {
            fid,
            parent_fid,
            name,
            name_lossy,
            full_path,
            size,
            allocated_size,
            is_dir: metadata.is_dir(),
            modified_at,
            attributes: 0,
        });
    }

    let duration = start.elapsed();
    tracing::info!(
        entries = entries.len(),
        duration_ms = duration.as_millis(),
        "jwalk fallback scan complete"
    );

    Ok(ScanResult {
        entries,
        cursor: None, // No USN journal on non-NTFS
        linux_cursor: None,
    })
}

/// Auto-detect filesystem and choose the best scan strategy.
///
/// - NTFS → MFT scan (fast, requires admin)
/// - FAT32/exFAT → jwalk fallback
/// - NTFS without admin → jwalk fallback
#[tracing::instrument(fields(volume = %volume.display()), skip(volume))]
pub fn auto_scan(volume: &Path) -> FsIndexerResult<ScanResult> {
    let fs_kind = detect::detect_filesystem(volume)?;

    match fs_kind {
        FilesystemKind::Ntfs => match full_scan_inner(volume, fs_kind) {
            Ok(result) => Ok(result),
            Err(FsIndexerError::MftAccess { volume: v, .. }) => {
                tracing::warn!(
                    volume = %v,
                    "MFT access denied, falling back to jwalk"
                );
                fallback_scan(volume)
            }
            Err(e) => Err(e),
        },
        FilesystemKind::Fat32 | FilesystemKind::ExFat => {
            tracing::info!(fs = ?fs_kind, "non-NTFS volume, using jwalk fallback");
            fallback_scan(volume)
        }
        _ => {
            tracing::info!(fs = ?fs_kind, "unknown filesystem, attempting jwalk fallback");
            fallback_scan(volume)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_synthetic_fid_high_enough() {
        // Ensure synthetic FIDs don't collide with real NTFS FRNs
        assert!(MIN_SYNTHETIC_FID > 1_000_000);
    }

    /// Full scan integration test — requires admin + NTFS.
    /// `cargo test -p hyprdrive-fs-indexer -- --ignored full_scan`
    #[test]
    #[ignore]
    fn full_scan_c_drive() {
        let result = full_scan(Path::new("C:\\"));
        match result {
            Ok(scan) => {
                assert!(
                    scan.entries.len() > 10_000,
                    "expected > 10k entries, got {}",
                    scan.entries.len()
                );

                // Check that sizes were enriched
                let files_with_size: usize = scan
                    .entries
                    .iter()
                    .filter(|e| !e.is_dir && e.size > 0)
                    .count();
                assert!(
                    files_with_size > 1000,
                    "expected > 1000 files with size > 0, got {}",
                    files_with_size
                );
            }
            Err(e) => {
                eprintln!("full_scan failed (expected without admin): {e}");
            }
        }
    }

    /// jwalk fallback test — doesn't require admin.
    /// `cargo test -p hyprdrive-fs-indexer -- --ignored fallback_scan`
    #[test]
    #[ignore]
    fn fallback_scan_temp_dir() {
        let dir = tempfile::TempDir::new().expect("create tempdir failed");
        // Create some test files
        for i in 0..10 {
            std::fs::write(
                dir.path().join(format!("file_{i}.txt")),
                format!("content {i}"),
            )
            .expect("write failed");
        }

        let result = fallback_scan(dir.path());
        assert!(result.is_ok(), "fallback_scan failed: {:?}", result);
        let scan = result.expect("already checked");
        // At least the 10 files we created + the temp dir itself
        assert!(
            scan.entries.len() >= 10,
            "expected >= 10 entries, got {}",
            scan.entries.len()
        );
    }

    /// Requires admin privileges on a real NTFS volume.
    /// Run: `cargo test -p hyprdrive-fs-indexer -- --ignored full_scan_enrichment_check`
    #[test]
    #[ignore]
    fn full_scan_enrichment_check() {
        let result = full_scan(Path::new("C:\\")).expect("full_scan should succeed as admin");
        let total = result.entries.len();
        let files = result.entries.iter().filter(|e| !e.is_dir).count();
        let with_size = result
            .entries
            .iter()
            .filter(|e| !e.is_dir && e.size > 0)
            .count();
        let deep_paths = result
            .entries
            .iter()
            .filter(|e| e.full_path.components().count() > 3)
            .count();

        println!(
            "Total: {total}, Files: {files}, With size: {with_size}, Deep paths: {deep_paths}"
        );

        assert!(
            with_size > files / 2,
            "expected >50% of files to have size, got {with_size}/{files}"
        );
        assert!(
            deep_paths > total / 2,
            "expected >50% of entries with depth > 3, got {deep_paths}/{total}"
        );
    }
}
