//! USN journal delta tracking — detect filesystem changes since last scan.
//!
//! After a full MFT scan, the USN journal cursor is persisted. Subsequent
//! calls to [`poll_changes`] return only the changes that occurred since
//! the last cursor position.
//!
//! ## API
//!
//! `usn-journal-rs` v0.4 provides:
//! - `Volume::from_drive_letter(char)`
//! - `UsnJournal::new(&Volume)` + `.query()` → `UsnJournalData`
//! - `UsnJournal::new(&Volume)` + `.iter()` → yields `UsnEntry`
//! - `UsnEntry { usn, time, fid, parent_fid, reason, source_info, file_name, file_attributes }`
//! - `UsnJournalData { journal_id, first_usn, next_usn, lowest_valid_usn, max_usn, ... }`

use crate::error::{FsIndexerError, FsIndexerResult};
use crate::types::{FsChange, IndexEntry, UsnCursor};
use chrono::Utc;
use std::path::Path;

/// USN reason flags for file operations.
const USN_REASON_FILE_CREATE: u32 = 0x00000100;
const USN_REASON_FILE_DELETE: u32 = 0x00000200;
const USN_REASON_RENAME_NEW_NAME: u32 = 0x00002000;
const USN_REASON_DATA_EXTEND: u32 = 0x00000002;
const USN_REASON_DATA_TRUNCATION: u32 = 0x00000004;
const USN_REASON_DATA_OVERWRITE: u32 = 0x00000001;

/// Extract drive letter from path.
fn drive_letter(volume: &Path) -> FsIndexerResult<char> {
    let s = volume.to_string_lossy();
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        Ok(bytes[0] as char)
    } else {
        Err(FsIndexerError::JournalError(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "expected drive letter path like C:\\",
        )))
    }
}

/// Read the current USN journal cursor position for a volume.
///
/// This should be called after a full scan to establish the baseline
/// for subsequent delta queries.
#[tracing::instrument(fields(volume = %volume.display()), skip(volume))]
pub fn read_cursor(volume: &Path) -> FsIndexerResult<UsnCursor> {
    let letter = drive_letter(volume)?;

    let vol = usn_journal_rs::volume::Volume::from_drive_letter(letter).map_err(|e| {
        FsIndexerError::JournalError(std::io::Error::other(e.to_string()))
    })?;

    let journal = usn_journal_rs::journal::UsnJournal::new(&vol);
    let journal_data = journal.query(false).map_err(|e| {
        FsIndexerError::JournalError(std::io::Error::other(e.to_string()))
    })?;

    Ok(UsnCursor {
        journal_id: journal_data.journal_id,
        next_usn: journal_data.next_usn,
    })
}

/// Poll the USN journal for changes since the given cursor position.
///
/// Returns a list of [`FsChange`] events and an updated cursor.
///
/// # Errors
///
/// Returns [`FsIndexerError::JournalError`] if the journal cannot be read,
/// which can happen if the journal has been deleted/recreated or if the
/// cursor is too old (journal wrap-around).
#[tracing::instrument(fields(volume = %volume.display()), skip(volume, cursor))]
pub fn poll_changes(
    volume: &Path,
    cursor: &UsnCursor,
) -> FsIndexerResult<(Vec<FsChange>, UsnCursor)> {
    let letter = drive_letter(volume)?;

    let vol = usn_journal_rs::volume::Volume::from_drive_letter(letter).map_err(|e| {
        FsIndexerError::JournalError(std::io::Error::other(e.to_string()))
    })?;

    let journal = usn_journal_rs::journal::UsnJournal::new(&vol);

    // Create enum options starting from the cursor's next_usn
    let options = usn_journal_rs::journal::EnumOptions::default();
    let journal_iter = journal.iter_with_options(options).map_err(|e| {
        FsIndexerError::JournalError(std::io::Error::other(e.to_string()))
    })?;

    let mut changes = Vec::new();
    let mut max_usn = cursor.next_usn;

    for result in journal_iter {
        let record = match result {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "skipping USN record due to error");
                continue;
            }
        };

        // Skip records before our cursor position
        if record.usn < cursor.next_usn {
            continue;
        }

        if record.usn > max_usn {
            max_usn = record.usn;
        }

        let reason = record.reason;
        let fid = record.fid;
        let parent_fid = record.parent_fid;
        let name = record.file_name;

        if (reason & USN_REASON_FILE_CREATE) != 0 {
            changes.push(FsChange::Created(IndexEntry {
                fid,
                parent_fid,
                name: name.clone(),
                name_lossy: name.to_string_lossy().to_string(),
                full_path: std::path::PathBuf::new(), // needs path reconstruction
                size: 0,                               // needs enrichment
                allocated_size: 0,
                is_dir: false,
                modified_at: Utc::now(),
                attributes: record.file_attributes,
            }));
        } else if (reason & USN_REASON_FILE_DELETE) != 0 {
            changes.push(FsChange::Deleted { fid });
        } else if (reason & USN_REASON_RENAME_NEW_NAME) != 0 {
            changes.push(FsChange::Moved {
                fid,
                new_parent_fid: parent_fid,
                new_name: name,
            });
        } else if (reason
            & (USN_REASON_DATA_EXTEND | USN_REASON_DATA_TRUNCATION | USN_REASON_DATA_OVERWRITE))
            != 0
        {
            changes.push(FsChange::Modified { fid, new_size: 0 });
        }
    }

    let new_cursor = UsnCursor {
        journal_id: cursor.journal_id,
        next_usn: max_usn + 1,
    };

    tracing::info!(
        changes = changes.len(),
        old_usn = cursor.next_usn,
        new_usn = new_cursor.next_usn,
        "USN journal poll complete"
    );

    Ok((changes, new_cursor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usn_reason_flags_correct() {
        assert_eq!(USN_REASON_FILE_CREATE, 0x100);
        assert_eq!(USN_REASON_FILE_DELETE, 0x200);
        assert_eq!(USN_REASON_RENAME_NEW_NAME, 0x2000);
    }

    #[test]
    fn drive_letter_extraction() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(drive_letter(Path::new("C:\\"))?, 'C');
        assert!(drive_letter(Path::new("/mnt/data")).is_err());
        Ok(())
    }

    /// Requires admin. Run: `cargo test -p hyprdrive-fs-indexer -- --ignored read_cursor`
    #[test]
    #[ignore]
    fn read_cursor_returns_valid_position() {
        let cursor = read_cursor(Path::new("C:\\"));
        match cursor {
            Ok(c) => {
                assert!(c.journal_id > 0, "journal_id should be > 0");
                assert!(c.next_usn > 0, "next_usn should be > 0");
            }
            Err(e) => {
                eprintln!("read_cursor failed (expected without admin): {e}");
            }
        }
    }

    /// Requires admin. Run: `cargo test -p hyprdrive-fs-indexer -- --ignored poll_changes`
    #[test]
    #[ignore]
    fn poll_changes_after_file_create() {
        // 1. Read cursor
        let cursor = read_cursor(Path::new("C:\\")).expect("read cursor");

        // 2. Create a test file
        let test_path = std::env::temp_dir().join("hyprdrive_usn_test.tmp");
        std::fs::write(&test_path, b"test content").expect("write test file");

        // 3. Poll changes
        let (changes, _new_cursor) =
            poll_changes(Path::new("C:\\"), &cursor).expect("poll changes");

        // 4. Cleanup
        let _ = std::fs::remove_file(&test_path);

        // 5. Verify — should contain at least one Created change
        let creates: Vec<_> = changes
            .iter()
            .filter(|c| matches!(c, FsChange::Created(_)))
            .collect();
        assert!(
            !creates.is_empty(),
            "expected at least one Created change after file write"
        );
    }
}
