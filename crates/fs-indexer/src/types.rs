//! Core types for the filesystem indexer.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::PathBuf;

/// A single indexed filesystem entry (file or directory).
///
/// Built in two phases on NTFS:
/// 1. Topology pass (MFT enumeration) → fid, parent_fid, name, is_dir, attributes
/// 2. Enrichment pass (GetFileInformationByHandleEx) → size, allocated_size, modified_at
#[derive(Debug, Clone)]
pub struct IndexEntry {
    /// File Reference Number (NTFS) or synthetic ID (jwalk fallback).
    pub fid: u64,
    /// Parent directory's FRN.
    pub parent_fid: u64,
    /// Filesystem-native name (preserves full Unicode fidelity).
    pub name: OsString,
    /// Lossy UTF-8 name for DB insert and display.
    pub name_lossy: String,
    /// Full path from volume root (built from parent chain).
    pub full_path: PathBuf,
    /// Logical file size in bytes (EOF position).
    pub size: u64,
    /// On-disk allocated size in bytes (may differ for compressed/sparse files).
    pub allocated_size: u64,
    /// Whether this entry is a directory.
    pub is_dir: bool,
    /// Last modification timestamp.
    pub modified_at: DateTime<Utc>,
    /// Win32 FILE_ATTRIBUTE_* flags (0 on non-Windows).
    pub attributes: u32,
}

/// A filesystem change detected via USN journal or re-walk diffing.
#[derive(Debug, Clone)]
pub enum FsChange {
    /// A new file or directory was created.
    Created(IndexEntry),
    /// A file or directory was deleted.
    Deleted {
        /// FRN of the deleted entry.
        fid: u64,
    },
    /// A file or directory was moved or renamed.
    Moved {
        /// FRN of the moved entry.
        fid: u64,
        /// New parent directory FRN.
        new_parent_fid: u64,
        /// New name after the move.
        new_name: OsString,
    },
    /// A file's content was modified (size may have changed).
    Modified {
        /// FRN of the modified entry.
        fid: u64,
        /// New logical size after modification.
        new_size: u64,
    },
    /// A full rescan is needed (e.g. USN journal wrapped or journal_id changed).
    FullRescanNeeded {
        /// Volume path that needs rescanning.
        volume: PathBuf,
        /// Human-readable reason for the rescan.
        reason: String,
    },
}

/// Detected filesystem type for a volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilesystemKind {
    /// NTFS — supports MFT enumeration and USN journal.
    Ntfs,
    /// FAT32 — requires jwalk fallback.
    Fat32,
    /// exFAT — requires jwalk fallback.
    ExFat,
    /// ReFS — Windows resilient filesystem.
    Refs,
    /// Unknown or unsupported filesystem.
    Unknown,
}

/// Topology entry from MFT enumeration (phase 1, no sizes).
#[derive(Debug, Clone)]
pub struct TopoEntry {
    /// File Reference Number.
    pub fid: u64,
    /// Parent directory FRN.
    pub parent_fid: u64,
    /// Filesystem-native filename.
    pub name: OsString,
    /// Whether this is a directory.
    pub is_dir: bool,
    /// Win32 file attributes.
    pub attributes: u32,
}

/// USN journal cursor for tracking delta position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsnCursor {
    /// The USN journal ID.
    pub journal_id: u64,
    /// The next USN to read from.
    pub next_usn: i64,
}

/// Volume scan result combining entries with cursor state.
#[derive(Debug)]
pub struct ScanResult {
    /// All indexed entries from the scan.
    pub entries: Vec<IndexEntry>,
    /// USN cursor for subsequent delta queries (None for non-NTFS).
    pub cursor: Option<UsnCursor>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filesystem_kind_serde_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let kind = FilesystemKind::Ntfs;
        let json = serde_json::to_string(&kind)?;
        let back: FilesystemKind = serde_json::from_str(&json)?;
        assert_eq!(kind, back);
        Ok(())
    }

    #[test]
    fn usn_cursor_serde_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let cursor = UsnCursor {
            journal_id: 42,
            next_usn: 1000,
        };
        let json = serde_json::to_string(&cursor)?;
        let back: UsnCursor = serde_json::from_str(&json)?;
        assert_eq!(cursor.journal_id, back.journal_id);
        assert_eq!(cursor.next_usn, back.next_usn);
        Ok(())
    }

    #[test]
    fn fs_change_full_rescan_needed() {
        let change = FsChange::FullRescanNeeded {
            volume: PathBuf::from("C:\\"),
            reason: "USN journal wrapped".to_string(),
        };
        match &change {
            FsChange::FullRescanNeeded { volume, reason } => {
                assert_eq!(volume, &PathBuf::from("C:\\"));
                assert_eq!(reason, "USN journal wrapped");
            }
            _ => panic!("expected FullRescanNeeded"),
        }
    }

    #[test]
    fn index_entry_lossy_name_matches() {
        let entry = IndexEntry {
            fid: 1,
            parent_fid: 0,
            name: OsString::from("test.txt"),
            name_lossy: "test.txt".to_string(),
            full_path: PathBuf::from("C:\\test.txt"),
            size: 100,
            allocated_size: 4096,
            is_dir: false,
            modified_at: Utc::now(),
            attributes: 0,
        };
        assert_eq!(entry.name_lossy, entry.name.to_string_lossy());
    }
}
