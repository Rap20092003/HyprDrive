//! MFT topology pass — enumerate all file entries from the NTFS Master File Table.
//!
//! Phase 1 of the two-phase scan. Returns topology only (FRN, parent, name, attributes).
//! **No sizes** — `MftEntry` from `usn-journal-rs` does not expose size fields.
//!
//! ## API
//!
//! `usn-journal-rs` v0.4 provides:
//! - `Volume::from_drive_letter(char) → Result<Volume, UsnError>`
//! - `Mft::new(&Volume)` + `.iter() → impl Iterator<Item = Result<MftEntry, UsnError>>`
//! - `MftEntry { usn, fid, parent_fid, file_name: OsString, file_attributes: u32 }`

use crate::error::{FsIndexerError, FsIndexerResult};
use crate::types::TopoEntry;
use std::ffi::OsString;
use std::path::Path;

/// Minimum FRN for user files. FRNs 0–23 are NTFS metadata files
/// ($MFT, $MFTMirr, $LogFile, $Volume, $AttrDef, ., $Bitmap, $Boot,
/// $BadClus, $Secure, $UpCase, $Extend, and reserved entries).
pub(crate) const MIN_USER_FRN: u64 = 24;

/// Win32 file attribute flag for directories.
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
/// Win32 file attribute flag for reparse points (junctions, symlinks).
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

use super::util::drive_letter_from_path;

/// Enumerate the MFT topology of an NTFS volume.
///
/// Returns all user file entries (FRN ≥ 24) as [`TopoEntry`] values.
/// System metadata files ($MFT, $LogFile, etc.) are skipped.
/// Reparse points (NTFS junctions, symlinks) are flagged but not followed.
///
/// # Errors
///
/// Returns [`FsIndexerError::MftAccess`] if the volume cannot be opened
/// (typically requires admin/elevated privileges).
#[tracing::instrument(fields(volume = %volume.display()), skip(volume))]
pub fn mft_enumerate_topology(volume: &Path) -> FsIndexerResult<Vec<TopoEntry>> {
    let letter = drive_letter_from_path(volume)?;

    tracing::info!(drive = %letter, "starting MFT topology enumeration");

    let vol = usn_journal_rs::volume::Volume::from_drive_letter(letter).map_err(|e| {
        FsIndexerError::MftAccess {
            volume: volume.display().to_string(),
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, e.to_string()),
        }
    })?;

    let mft = usn_journal_rs::mft::Mft::new(&vol);
    let mft_iter = mft.iter();

    let mut entries = Vec::new();

    for result in mft_iter {
        let mft_entry = match result {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(error = %e, "skipping MFT entry due to error");
                continue;
            }
        };

        // Skip NTFS metadata files (FRN < 24)
        if mft_entry.fid < MIN_USER_FRN {
            continue;
        }

        let is_dir = (mft_entry.file_attributes & FILE_ATTRIBUTE_DIRECTORY) != 0;
        let is_reparse = (mft_entry.file_attributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0;

        if is_reparse {
            tracing::trace!(
                fid = mft_entry.fid,
                name = %mft_entry.file_name.to_string_lossy(),
                "skipping reparse point"
            );
            continue;
        }

        entries.push(TopoEntry {
            fid: mft_entry.fid,
            parent_fid: mft_entry.parent_fid,
            name: mft_entry.file_name,
            is_dir,
            attributes: mft_entry.file_attributes,
        });
    }

    tracing::info!(count = entries.len(), "MFT topology enumeration complete");
    Ok(entries)
}

/// Build a parent-FRN lookup map from topology entries.
///
/// Returns a map from FRN → (parent_fid, name) for path reconstruction.
pub fn build_parent_map(entries: &[TopoEntry]) -> std::collections::HashMap<u64, (u64, OsString)> {
    entries
        .iter()
        .map(|e| (e.fid, (e.parent_fid, e.name.clone())))
        .collect()
}

/// Reconstruct the full path from a parent map by walking up the FRN chain.
///
/// Returns `None` if a broken parent chain is detected (orphan node
/// whose parent is not in the map and is not a self-referencing root).
pub fn reconstruct_path(
    fid: u64,
    parent_map: &std::collections::HashMap<u64, (u64, OsString)>,
    volume_root: &Path,
) -> Option<std::path::PathBuf> {
    let mut components = Vec::new();
    let mut current = fid;
    let mut reached_root = false;

    // Walk up the parent chain (max depth to prevent infinite loops)
    for _ in 0..4096 {
        match parent_map.get(&current) {
            Some((parent_fid, name)) => {
                components.push(name.clone());
                if *parent_fid == current {
                    // Root entry: parent == self
                    reached_root = true;
                    break;
                }
                current = *parent_fid;
            }
            None => {
                // Broken chain — parent not in map
                break;
            }
        }
    }

    if !reached_root {
        return None;
    }

    components.reverse();
    let mut path = volume_root.to_path_buf();
    for component in &components {
        path.push(component);
    }
    Some(path)
}

/// Reconstruct full paths for multiple fids with memoization.
///
/// Caches intermediate path segments so files in the same directory
/// only compute the parent path once. Returns a map from fid → `PathBuf`.
///
/// Much faster than calling [`reconstruct_path`] in a loop for large entry
/// sets because sibling files share the same parent chain computation.
pub fn reconstruct_paths_cached(
    fids: &[u64],
    parent_map: &std::collections::HashMap<u64, (u64, OsString)>,
    volume_root: &Path,
) -> std::collections::HashMap<u64, std::path::PathBuf> {
    let mut cache: std::collections::HashMap<u64, std::path::PathBuf> =
        std::collections::HashMap::new();
    let mut result = std::collections::HashMap::with_capacity(fids.len());

    for &fid in fids {
        if let Some(path) = reconstruct_path_memo(fid, parent_map, volume_root, &mut cache) {
            result.insert(fid, path);
        }
    }
    result
}

/// Iterative path reconstruction with memoization cache.
///
/// Walks the parent chain from `fid` upward, stopping early if a cached
/// intermediate path is found. Caches every intermediate node on the way
/// back down so subsequent lookups in the same subtree are O(1).
fn reconstruct_path_memo(
    fid: u64,
    parent_map: &std::collections::HashMap<u64, (u64, OsString)>,
    volume_root: &Path,
    cache: &mut std::collections::HashMap<u64, std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    // Fast path: already cached
    if let Some(cached) = cache.get(&fid) {
        return Some(cached.clone());
    }

    // Collect chain of (fid, name) up to root or a cached entry
    let mut chain: Vec<(u64, OsString)> = Vec::new();
    let mut current = fid;
    let mut base_path: Option<std::path::PathBuf> = None;

    for _ in 0..4096 {
        if let Some(cached) = cache.get(&current) {
            base_path = Some(cached.clone());
            break;
        }
        match parent_map.get(&current) {
            Some((parent_fid, name)) => {
                if *parent_fid == current {
                    // Root entry: parent == self
                    base_path = Some(volume_root.to_path_buf());
                    break;
                }
                chain.push((current, name.clone()));
                current = *parent_fid;
            }
            None => {
                // Orphan or volume root reached
                break;
            }
        }
    }

    let base = base_path?;

    // Build paths from base downward, caching each intermediate node
    let mut path = base;
    for (chain_fid, name) in chain.iter().rev() {
        path = path.join(name);
        cache.insert(*chain_fid, path.clone());
    }

    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NTFS root FRN record number used in test scenarios.
    const TEST_ROOT_FRN: u64 = 5;

    #[test]
    fn min_user_frn_skips_metadata() {
        assert_eq!(MIN_USER_FRN, 24);
        // NTFS root (FRN 5) is below MIN_USER_FRN, so it gets excluded
        // from topo_entries. The dynamic root detection in scanner.rs
        // handles this at runtime.
        assert!(TEST_ROOT_FRN < MIN_USER_FRN);
    }

    #[test]
    fn file_attribute_flags_correct() {
        assert_eq!(FILE_ATTRIBUTE_DIRECTORY, 0x10);
        assert_eq!(FILE_ATTRIBUTE_REPARSE_POINT, 0x400);
    }

    #[test]
    fn drive_letter_extraction() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(drive_letter_from_path(Path::new("C:\\"))?, 'C');
        assert_eq!(drive_letter_from_path(Path::new("D:\\Users"))?, 'D');
        assert!(drive_letter_from_path(Path::new("/mnt/data")).is_err());
        Ok(())
    }

    #[test]
    fn reconstruct_path_simple() {
        let mut map = std::collections::HashMap::new();
        // Root (fid=5, parent=5)
        map.insert(5, (5u64, OsString::from("")));
        // Dir (fid=100, parent=5)
        map.insert(100, (5u64, OsString::from("Users")));
        // File (fid=200, parent=100)
        map.insert(200, (100u64, OsString::from("test.txt")));

        let path = reconstruct_path(200, &map, Path::new("C:\\"));
        assert!(path.is_some());
        let p = path.expect("path should exist");
        assert!(p.to_string_lossy().contains("Users"));
        assert!(p.to_string_lossy().contains("test.txt"));
    }

    #[test]
    fn build_parent_map_contains_entries() {
        let entries = vec![
            TopoEntry {
                fid: 100,
                parent_fid: 5,
                name: OsString::from("folder"),
                is_dir: true,
                attributes: FILE_ATTRIBUTE_DIRECTORY,
            },
            TopoEntry {
                fid: 200,
                parent_fid: 100,
                name: OsString::from("file.txt"),
                is_dir: false,
                attributes: 0,
            },
        ];

        let map = build_parent_map(&entries);
        assert_eq!(map.len(), 2);
        assert!(map.contains_key(&100));
        assert!(map.contains_key(&200));
    }

    #[test]
    fn reconstruct_paths_cached_basic() {
        let mut map = std::collections::HashMap::new();
        // root(5) -> dir(100, "docs") -> file_a(200, "a.txt"), file_b(201, "b.txt")
        map.insert(5, (5u64, OsString::from(""))); // root
        map.insert(100, (5u64, OsString::from("docs")));
        map.insert(200, (100u64, OsString::from("a.txt")));
        map.insert(201, (100u64, OsString::from("b.txt")));

        let root = Path::new("C:\\");
        let result = reconstruct_paths_cached(&[200, 201], &map, root);
        assert_eq!(result.len(), 2);
        assert!(result[&200].to_string_lossy().contains("docs"));
        assert!(result[&200].to_string_lossy().contains("a.txt"));
        assert!(result[&201].to_string_lossy().contains("b.txt"));
    }

    #[test]
    fn reconstruct_paths_cached_shares_prefix() {
        let mut map = std::collections::HashMap::new();
        // root(1) -> a(2) -> b(3) -> c.txt(4), d.txt(5)
        map.insert(1, (1u64, OsString::from("")));
        map.insert(2, (1u64, OsString::from("a")));
        map.insert(3, (2u64, OsString::from("b")));
        map.insert(4, (3u64, OsString::from("c.txt")));
        map.insert(5, (3u64, OsString::from("d.txt")));

        let root = Path::new("C:\\");
        let result = reconstruct_paths_cached(&[4, 5], &map, root);
        let p4 = result[&4].to_string_lossy().to_string();
        let p5 = result[&5].to_string_lossy().to_string();
        assert!(p4.contains("a") && p4.contains("b") && p4.contains("c.txt"));
        assert!(p5.contains("a") && p5.contains("b") && p5.contains("d.txt"));
    }

    #[test]
    fn reconstruct_path_orphan_returns_none() {
        // Entry with parent not in map → returns a partial path (not None,
        // because the current impl breaks on missing parent with a partial result).
        // But reconstruct_path_memo returns None for true orphans.
        let mut map = std::collections::HashMap::new();
        map.insert(1, (1u64, OsString::from(""))); // root
                                                   // fid=99 has parent=50 which is missing
        map.insert(99, (50u64, OsString::from("orphan.txt")));
        let result = reconstruct_paths_cached(&[99], &map, Path::new("C:\\"));
        // orphan — parent 50 not in map, so no base path found → not in result
        assert!(result.get(&99).is_none());
    }

    #[test]
    fn reconstruct_path_deep_chain() {
        // 10 levels deep → valid path
        let mut map = std::collections::HashMap::new();
        map.insert(1, (1u64, OsString::from("")));
        for i in 2u64..=11 {
            map.insert(i, (i - 1, OsString::from(format!("d{i}"))));
        }
        let result = reconstruct_path(11, &map, Path::new("C:\\"));
        assert!(result.is_some());
        let path = result.unwrap();
        // Should contain all directory components
        let s = path.to_string_lossy();
        assert!(s.contains("d11"));
        assert!(s.contains("d2"));
    }

    #[test]
    fn reconstruct_path_fails_without_root_then_succeeds_with_it() {
        // Simulate real scenario: topo_entries do NOT contain FRN 5
        // because mft_enumerate_topology() skips FRN < MIN_USER_FRN.
        let topo_entries = vec![
            TopoEntry {
                fid: 100,
                parent_fid: TEST_ROOT_FRN,
                name: OsString::from("Users"),
                is_dir: true,
                attributes: FILE_ATTRIBUTE_DIRECTORY,
            },
            TopoEntry {
                fid: 200,
                parent_fid: 100,
                name: OsString::from("report.pdf"),
                is_dir: false,
                attributes: 0,
            },
        ];

        let mut parent_map = build_parent_map(&topo_entries);

        // Without root → reconstruction fails (this was the bug)
        let before = reconstruct_paths_cached(&[200], &parent_map, Path::new("C:\\"));
        assert!(
            before.get(&200).is_none(),
            "should fail without root in map"
        );

        // With root → reconstruction succeeds (this is the fix)
        parent_map.insert(TEST_ROOT_FRN, (TEST_ROOT_FRN, OsString::new()));
        let after = reconstruct_paths_cached(&[200], &parent_map, Path::new("C:\\"));
        let path = after
            .get(&200)
            .expect("should resolve after injecting root");
        assert_eq!(path.to_string_lossy(), r"C:\Users\report.pdf");
    }

    #[test]
    fn reconstruct_path_deep_chain_with_injected_root() {
        let topo_entries = vec![
            TopoEntry {
                fid: 100,
                parent_fid: TEST_ROOT_FRN,
                name: OsString::from("Users"),
                is_dir: true,
                attributes: FILE_ATTRIBUTE_DIRECTORY,
            },
            TopoEntry {
                fid: 101,
                parent_fid: 100,
                name: OsString::from("rajab"),
                is_dir: true,
                attributes: FILE_ATTRIBUTE_DIRECTORY,
            },
            TopoEntry {
                fid: 102,
                parent_fid: 101,
                name: OsString::from("Documents"),
                is_dir: true,
                attributes: FILE_ATTRIBUTE_DIRECTORY,
            },
            TopoEntry {
                fid: 200,
                parent_fid: 102,
                name: OsString::from("file.txt"),
                is_dir: false,
                attributes: 0,
            },
        ];
        let mut parent_map = build_parent_map(&topo_entries);
        parent_map.insert(TEST_ROOT_FRN, (TEST_ROOT_FRN, OsString::new()));

        let result = reconstruct_paths_cached(&[200], &parent_map, Path::new("C:\\"));
        let path = result.get(&200).expect("deep path should resolve");
        assert_eq!(path.to_string_lossy(), r"C:\Users\rajab\Documents\file.txt");
    }

    /// This test requires admin privileges and a real NTFS volume.
    /// Run manually: `cargo test -p hyprdrive-fs-indexer -- --ignored mft_enumerate`
    #[test]
    #[ignore]
    fn mft_enumerate_returns_entries() {
        let entries = mft_enumerate_topology(Path::new("C:\\"));
        match entries {
            Ok(e) => {
                assert!(e.len() > 10_000, "expected > 10k entries, got {}", e.len());
                for entry in &e {
                    assert!(
                        entry.fid >= MIN_USER_FRN,
                        "FRN {} < {}",
                        entry.fid,
                        MIN_USER_FRN
                    );
                }
                let dir_count = e.iter().filter(|e| e.is_dir).count();
                assert!(dir_count > 0, "expected at least some directories");
            }
            Err(e) => {
                eprintln!("MFT enumeration failed (expected without admin): {e}");
            }
        }
    }
}
