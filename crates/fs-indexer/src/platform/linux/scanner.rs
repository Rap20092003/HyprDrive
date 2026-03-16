//! Linux scan orchestrator — composes detect + walk + enrich.
//!
//! Mirrors the Windows scanner architecture:
//! - [`full_scan`] — detect filesystem → walk topology → enrich sizes
//! - [`fallback_scan`] — same as full_scan (both use jwalk; io_uring upgrade later)
//! - [`auto_scan`] — detect filesystem → choose strategy → scan

use crate::error::{FsIndexerError, FsIndexerResult};
use crate::platform::linux::{detect, enrich, walk};
use crate::types::{FilesystemKind, IndexEntry, LinuxCursor, ScanResult, TopoEntry};
use chrono::Utc;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Build a map from fid → (parent_fid, name) for path reconstruction.
fn build_parent_map(entries: &[TopoEntry]) -> HashMap<u64, (u64, OsString)> {
    let mut map = HashMap::with_capacity(entries.len());
    for entry in entries {
        map.insert(entry.fid, (entry.parent_fid, entry.name.clone()));
    }
    map
}

/// Reconstruct full path from fid by walking parent chain.
///
/// Walks up the parent chain until it finds the root (where fid == parent_fid)
/// or runs out of entries, then builds the path from root down.
fn reconstruct_path(
    fid: u64,
    parent_map: &HashMap<u64, (u64, OsString)>,
    root: &Path,
) -> Option<PathBuf> {
    let mut components = Vec::new();
    let mut current = fid;
    let mut visited = std::collections::HashSet::new();

    loop {
        if !visited.insert(current) {
            // Cycle detection
            break;
        }

        if let Some((parent_fid, name)) = parent_map.get(&current) {
            components.push(name.clone());
            if *parent_fid == current {
                // Reached root (self-referencing parent)
                break;
            }
            current = *parent_fid;
        } else {
            break;
        }
    }

    components.reverse();

    // The first component is the root dir itself — skip it and build from root
    if components.len() <= 1 {
        // This is the root entry itself
        Some(root.to_path_buf())
    } else {
        let mut path = root.to_path_buf();
        for component in &components[1..] {
            path.push(component);
        }
        Some(path)
    }
}

/// Perform a full Linux filesystem scan.
///
/// 1. Detects filesystem type (returns error for pseudo-filesystems)
/// 2. Walks directory tree (jwalk + inode capture)
/// 3. Converts [`TopoEntry`] → [`IndexEntry`] with path reconstruction
/// 4. Enriches sizes via `lstat()`
/// 5. Returns [`ScanResult`] with [`LinuxCursor`]
#[tracing::instrument(fields(root = %root.display()), skip(root))]
pub fn full_scan(root: &Path) -> FsIndexerResult<ScanResult> {
    let start = std::time::Instant::now();

    // Phase 0: Detect filesystem
    let fs_kind = detect::detect_filesystem(root)?;
    if fs_kind == FilesystemKind::NineP {
        tracing::warn!(
            "9p mount detected — consider indexing from the Windows side for better performance"
        );
    }

    // Phase 1: Walk topology
    let topo_entries = walk::walk_directory(root, true)?;
    let topo_count = topo_entries.len();
    tracing::info!(entries = topo_count, "topology pass complete");

    // Build parent map for path reconstruction
    let parent_map = build_parent_map(&topo_entries);

    // Phase 2: Convert to IndexEntry with paths
    let mut entries: Vec<IndexEntry> = topo_entries
        .into_iter()
        .map(|topo| {
            let full_path = reconstruct_path(topo.fid, &parent_map, root)
                .unwrap_or_else(|| root.join(&topo.name));
            let name_lossy = topo.name.to_string_lossy().to_string();

            IndexEntry {
                fid: topo.fid,
                parent_fid: topo.parent_fid,
                name: topo.name,
                name_lossy,
                full_path,
                size: 0,           // filled by enrichment
                allocated_size: 0, // filled by enrichment
                is_dir: topo.is_dir,
                modified_at: Utc::now(), // updated during enrichment
                attributes: topo.attributes,
            }
        })
        .collect();

    // Phase 3: Size enrichment
    enrich::enrich_sizes(&mut entries)?;

    let duration = start.elapsed();
    tracing::info!(
        entries = entries.len(),
        duration_ms = duration.as_millis(),
        fs = ?fs_kind,
        "full scan complete"
    );

    let cursor = LinuxCursor {
        last_scan_epoch_ms: Utc::now().timestamp_millis(),
        fanotify_active: false,
    };

    Ok(ScanResult {
        entries,
        cursor: None,
        linux_cursor: Some(cursor),
    })
}

/// Fallback scan — identical to [`full_scan`] for now.
///
/// When `io_uring` support is added, [`full_scan`] will use `io_uring`
/// and this function remains jwalk-based.
#[tracing::instrument(fields(root = %root.display()), skip(root))]
pub fn fallback_scan(root: &Path) -> FsIndexerResult<ScanResult> {
    full_scan(root)
}

/// Auto-detect filesystem and choose the best scan strategy.
///
/// - ext4/btrfs/XFS → [`full_scan`] (jwalk + inode capture)
/// - 9p → [`full_scan`] with warning
/// - Pseudo-fs → [`PseudoFilesystem`](FsIndexerError::PseudoFilesystem) error
/// - PermissionDenied → [`fallback_scan`]
#[tracing::instrument(fields(root = %root.display()), skip(root))]
pub fn auto_scan(root: &Path) -> FsIndexerResult<ScanResult> {
    // Let PseudoFilesystem error propagate
    let fs_kind = detect::detect_filesystem(root)?;

    tracing::info!(fs = ?fs_kind, "auto-detected filesystem, starting scan");

    match full_scan(root) {
        Ok(result) => Ok(result),
        Err(FsIndexerError::PermissionDenied { path }) => {
            tracing::warn!(
                path = %path,
                "permission denied during full scan, falling back"
            );
            fallback_scan(root)
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_scan_tempdir() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        for i in 0..5 {
            std::fs::write(
                dir.path().join(format!("file_{i}.txt")),
                format!("content {i}"),
            )
            .expect("write file");
        }

        let result = full_scan(dir.path()).expect("full scan should succeed");
        // 1 root dir + 5 files = 6
        assert_eq!(result.entries.len(), 6);

        // Files should have size > 0
        let files_with_size: usize = result
            .entries
            .iter()
            .filter(|e| !e.is_dir && e.size > 0)
            .count();
        assert_eq!(files_with_size, 5, "all 5 files should have size > 0");
    }

    #[test]
    fn full_scan_empty_dir() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        let result = full_scan(dir.path()).expect("scan should succeed");
        assert_eq!(result.entries.len(), 1, "empty dir → 1 entry (itself)");
        assert!(result.entries[0].is_dir);
    }

    #[test]
    fn full_scan_nested_dirs() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        std::fs::create_dir_all(dir.path().join("a").join("b").join("c")).expect("mkdir -p");
        std::fs::write(dir.path().join("a").join("b").join("c").join("deep.txt"), "nested")
            .expect("write");

        let result = full_scan(dir.path()).expect("scan should succeed");

        // Find the deep file
        let deep = result
            .entries
            .iter()
            .find(|e| e.name_lossy == "deep.txt");
        assert!(deep.is_some(), "should find deep.txt");
        assert_eq!(deep.expect("checked above").size, 6); // "nested" = 6 bytes

        // Verify parent chain: deep.txt → c → b → a → root
        let fid_map: HashMap<u64, &IndexEntry> =
            result.entries.iter().map(|e| (e.fid, e)).collect();
        let deep_entry = deep.expect("checked above");
        // Parent should be directory "c"
        if let Some(parent) = fid_map.get(&deep_entry.parent_fid) {
            assert_eq!(parent.name_lossy, "c");
            assert!(parent.is_dir);
        }
    }

    #[test]
    fn auto_scan_tempdir() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        std::fs::write(dir.path().join("test.txt"), "auto").expect("write");

        let result = auto_scan(dir.path()).expect("auto scan should succeed");
        assert!(result.entries.len() >= 2); // dir + file
    }

    #[test]
    fn scan_result_has_linux_cursor() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        let result = full_scan(dir.path()).expect("scan");

        assert!(result.linux_cursor.is_some(), "should have linux_cursor");
        assert!(result.cursor.is_none(), "should not have USN cursor");

        let cursor = result.linux_cursor.expect("checked above");
        assert!(!cursor.fanotify_active);
        assert!(cursor.last_scan_epoch_ms > 0);
    }

    #[test]
    #[ignore] // Requires Linux — run in WSL2
    fn auto_scan_root() {
        let result = auto_scan(Path::new("/"));
        assert!(result.is_ok(), "auto_scan of / should succeed");
        let scan = result.expect("checked above");
        assert!(
            scan.entries.len() > 100,
            "/ should have many entries, got {}",
            scan.entries.len()
        );
    }

    #[test]
    #[ignore] // Requires Linux — run in WSL2
    fn auto_scan_proc_fails() {
        let result = auto_scan(Path::new("/proc"));
        assert!(result.is_err(), "/proc should return PseudoFilesystem error");
        let err = result.expect_err("checked above");
        let err_str = format!("{err}");
        assert!(
            err_str.contains("pseudo-filesystem"),
            "error should mention pseudo-filesystem: {err_str}"
        );
    }
}
