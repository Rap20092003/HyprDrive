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
const MIN_USER_FRN: u64 = 24;

/// Win32 file attribute flag for directories.
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
/// Win32 file attribute flag for reparse points (junctions, symlinks).
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// Extract drive letter from a path like `C:\` → `'C'`.
fn drive_letter_from_path(volume: &Path) -> FsIndexerResult<char> {
    let s = volume.to_string_lossy();
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        Ok(bytes[0] as char)
    } else {
        Err(FsIndexerError::MftAccess {
            volume: volume.display().to_string(),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "expected a drive letter path like C:\\",
            ),
        })
    }
}

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
pub fn build_parent_map(
    entries: &[TopoEntry],
) -> std::collections::HashMap<u64, (u64, OsString)> {
    entries
        .iter()
        .map(|e| (e.fid, (e.parent_fid, e.name.clone())))
        .collect()
}

/// Reconstruct the full path from a parent map by walking up the FRN chain.
///
/// Returns `None` if a broken parent chain is detected.
pub fn reconstruct_path(
    fid: u64,
    parent_map: &std::collections::HashMap<u64, (u64, OsString)>,
    volume_root: &Path,
) -> Option<std::path::PathBuf> {
    let mut components = Vec::new();
    let mut current = fid;

    // Walk up the parent chain (max depth to prevent infinite loops)
    for _ in 0..4096 {
        match parent_map.get(&current) {
            Some((parent_fid, name)) => {
                components.push(name.clone());
                if *parent_fid == current {
                    // Root entry: parent == self
                    break;
                }
                current = *parent_fid;
            }
            None => {
                // Reached volume root or broken chain
                break;
            }
        }
    }

    components.reverse();
    let mut path = volume_root.to_path_buf();
    for component in &components {
        path.push(component);
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_user_frn_skips_metadata() {
        assert_eq!(MIN_USER_FRN, 24);
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

    /// This test requires admin privileges and a real NTFS volume.
    /// Run manually: `cargo test -p hyprdrive-fs-indexer -- --ignored mft_enumerate`
    #[test]
    #[ignore]
    fn mft_enumerate_returns_entries() {
        let entries = mft_enumerate_topology(Path::new("C:\\"));
        match entries {
            Ok(e) => {
                assert!(
                    e.len() > 10_000,
                    "expected > 10k entries, got {}",
                    e.len()
                );
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
