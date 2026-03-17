//! Directory enumeration using jwalk with inode capture.
//!
//! Walks a directory tree using [`jwalk`] for parallel traversal, capturing
//! Linux inode numbers (`st_ino`) and device IDs (`st_dev`) for stable
//! cross-scan file identification.

use crate::error::FsIndexerResult;
use crate::types::TopoEntry;
use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::detect;

/// Synthesize a stable fid from (device, inode) pair.
///
/// Uses a deterministic hash to combine full 64-bit device and inode values.
/// This avoids truncation of 64-bit inodes on XFS/btrfs, which would cause
/// collisions when inode numbers differ only in the upper 32 bits.
///
/// The hash is stable across process restarts (fixed seed, not randomized).
pub(crate) fn make_fid(dev: u64, ino: u64) -> u64 {
    // Use a simple but collision-resistant mixing function.
    // We XOR the device with a large prime then combine with the full inode.
    // This preserves all 64 bits of ino and mixes in dev.
    let dev_mixed = dev.wrapping_mul(0x517c_c1b7_2722_0a95); // large odd constant
    dev_mixed ^ ino
}

/// Walk a directory tree, returning topology entries with inode-based fids.
///
/// Uses [`jwalk`] for parallel directory walking. Captures `st_ino` and `st_dev`
/// from Unix metadata for stable cross-scan file identification.
///
/// # Arguments
/// * `root` — Directory to walk
/// * `skip_pseudo` — If true, skip pseudo-filesystems (/proc, /sys, /dev)
#[tracing::instrument(fields(root = %root.display(), skip_pseudo))]
pub fn walk_directory(root: &Path, skip_pseudo: bool) -> FsIndexerResult<Vec<TopoEntry>> {
    let root_meta = std::fs::metadata(root)?;
    let root_dev = root_meta.dev();
    let root_ino = root_meta.ino();
    let root_fid = make_fid(root_dev, root_ino);

    let mut entries = Vec::new();
    let mut dir_fid_map: HashMap<PathBuf, u64> = HashMap::new();
    // Track devices we've already warned about for 9p
    let mut warned_devices: std::collections::HashSet<u64> = std::collections::HashSet::new();

    // Register root directory
    dir_fid_map.insert(root.to_path_buf(), root_fid);

    for entry_result in jwalk::WalkDir::new(root)
        .skip_hidden(false)
        .follow_links(false)
    {
        let dir_entry = match entry_result {
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

        let dev = metadata.dev();
        let ino = metadata.ino();
        let fid = make_fid(dev, ino);
        let full_path = dir_entry.path();
        let is_dir = metadata.is_dir();

        // Skip pseudo-filesystems when crossing device boundaries
        if skip_pseudo && dev != root_dev {
            if detect::is_pseudo_filesystem(&full_path) {
                tracing::debug!(
                    path = %full_path.display(),
                    "skipping pseudo-filesystem"
                );
                continue;
            }
            // Warn about 9p mounts (only once per device)
            if warned_devices.insert(dev) {
                if let Ok(mount_info) = detect::parse_mount_info(&full_path) {
                    if mount_info.fs_type == "9p" {
                        tracing::warn!(
                            path = %full_path.display(),
                            "9p mount detected — indexing may be slow and \
                             inotify won't track Windows-side changes"
                        );
                    }
                }
            }
        }

        // Determine parent fid
        let parent_fid = if dir_entry.depth() == 0 {
            fid // root entry: parent is self
        } else {
            full_path
                .parent()
                .and_then(|p| dir_fid_map.get(p).copied())
                .unwrap_or(root_fid) // fallback to root
        };

        // Register directories for child parent lookups
        if is_dir {
            dir_fid_map.insert(full_path.clone(), fid);
        }

        // Detect symlinks via file_type (not metadata which follows symlinks)
        let is_symlink = dir_entry.file_type().is_symlink();
        /// Synthetic attribute flag for symlinks (no Win32 equivalent on Linux).
        const ATTR_SYMLINK: u32 = 1;
        let attributes: u32 = if is_symlink { ATTR_SYMLINK } else { 0 };

        entries.push(TopoEntry {
            fid,
            parent_fid,
            name: dir_entry.file_name().to_os_string(),
            is_dir: if is_symlink { false } else { is_dir },
            attributes,
        });
    }

    tracing::info!(entries = entries.len(), "directory walk complete");
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_fid_deterministic() {
        let fid1 = make_fid(1, 42);
        let fid2 = make_fid(1, 42);
        assert_eq!(fid1, fid2, "same inputs should produce same fid");
    }

    #[test]
    fn make_fid_preserves_64bit_inodes() {
        // Two inodes that differ only in upper 32 bits — must NOT collide
        let fid1 = make_fid(1, 0x1_0000_0001);
        let fid2 = make_fid(1, 0x2_0000_0001);
        assert_ne!(
            fid1, fid2,
            "64-bit inodes differing in upper bits must not collide"
        );
    }

    #[test]
    fn make_fid_different_devices() {
        let fid1 = make_fid(1, 100);
        let fid2 = make_fid(2, 100);
        assert_ne!(fid1, fid2, "same inode on different devices should differ");
    }

    #[test]
    fn walk_tempdir_returns_entries() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        for i in 0..5 {
            std::fs::write(
                dir.path().join(format!("file_{i}.txt")),
                format!("content {i}"),
            )
            .expect("write file");
        }
        let entries = walk_directory(dir.path(), false).expect("walk should succeed");
        // 1 root dir + 5 files = 6
        assert_eq!(
            entries.len(),
            6,
            "expected 6 entries, got {}",
            entries.len()
        );
    }

    #[test]
    fn walk_tempdir_inodes_nonzero() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        std::fs::write(dir.path().join("test.txt"), "hello").expect("write");
        let entries = walk_directory(dir.path(), false).expect("walk");
        for entry in &entries {
            assert_ne!(entry.fid, 0, "fid should be non-zero");
        }
    }

    #[test]
    fn walk_tempdir_parent_fids_valid() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        std::fs::create_dir(dir.path().join("subdir")).expect("mkdir");
        std::fs::write(dir.path().join("subdir").join("nested.txt"), "data").expect("write");
        let entries = walk_directory(dir.path(), false).expect("walk");

        // Build fid set
        let fid_set: std::collections::HashSet<u64> = entries.iter().map(|e| e.fid).collect();

        // All parent_fids should exist in the fid set
        for entry in &entries {
            assert!(
                fid_set.contains(&entry.parent_fid),
                "parent_fid {} not found for entry {:?}",
                entry.parent_fid,
                entry.name
            );
        }
    }

    #[test]
    fn walk_empty_dir() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        let entries = walk_directory(dir.path(), false).expect("walk");
        assert_eq!(entries.len(), 1, "empty dir should have 1 entry (itself)");
        assert!(entries[0].is_dir);
    }
}
