//! Named pipe client for communicating with the elevated helper process.
//!
//! The daemon calls these functions to delegate MFT/USN operations to the
//! helper binary running with admin privileges. If the helper is not running,
//! all functions return [`FsIndexerError::HelperUnavailable`].
//!
//! ## Connection Model
//!
//! Each function opens a fresh pipe connection, sends one request, reads one
//! response, then closes. This is simple and avoids connection state management.

use crate::error::{FsIndexerError, FsIndexerResult};
use crate::types::{FsChange, IndexEntry, ScanResult, UsnCursor};
use chrono::{DateTime, Utc};
use hyprdrive_ipc_protocol::{
    framing, HelperRequest, HelperResponse, WireChange, WireIndexEntry, PIPE_NAME,
};
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

/// Check if the helper pipe is available (helper is running).
///
/// Attempts to open the pipe for reading. Returns `true` if the pipe exists.
pub fn pipe_available() -> bool {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(PIPE_NAME)
        .is_ok()
}

/// Perform a full MFT scan via the helper pipe.
pub fn pipe_scan(volume: &Path) -> FsIndexerResult<ScanResult> {
    let request = HelperRequest::ScanVolume {
        volume: volume.to_string_lossy().to_string(),
    };

    let response = send_request(&request)?;

    match response {
        HelperResponse::ScanResult { entries, cursor } => {
            let index_entries: Vec<IndexEntry> = entries.into_iter().map(wire_to_entry).collect();
            let usn_cursor = cursor.map(|c| UsnCursor {
                journal_id: c.journal_id,
                next_usn: c.next_usn,
            });
            Ok(ScanResult {
                entries: index_entries,
                cursor: usn_cursor,
                linux_cursor: None,
            })
        }
        HelperResponse::Error { code, message } => Err(FsIndexerError::HelperError {
            code: format!("{:?}", code),
            message,
        }),
        other => Err(FsIndexerError::HelperError {
            code: "UnexpectedResponse".to_string(),
            message: format!(
                "expected ScanResult, got {:?}",
                std::mem::discriminant(&other)
            ),
        }),
    }
}

/// Read the USN journal cursor via the helper pipe.
pub fn pipe_read_cursor(volume: &Path) -> FsIndexerResult<UsnCursor> {
    let request = HelperRequest::ReadCursor {
        volume: volume.to_string_lossy().to_string(),
    };

    let response = send_request(&request)?;

    match response {
        HelperResponse::Cursor(c) => Ok(UsnCursor {
            journal_id: c.journal_id,
            next_usn: c.next_usn,
        }),
        HelperResponse::Error { code, message } => Err(FsIndexerError::HelperError {
            code: format!("{:?}", code),
            message,
        }),
        other => Err(FsIndexerError::HelperError {
            code: "UnexpectedResponse".to_string(),
            message: format!("expected Cursor, got {:?}", std::mem::discriminant(&other)),
        }),
    }
}

/// Poll USN journal changes via the helper pipe.
pub fn pipe_poll_changes(
    volume: &Path,
    cursor: &UsnCursor,
) -> FsIndexerResult<(Vec<FsChange>, UsnCursor)> {
    let request = HelperRequest::PollChanges {
        volume: volume.to_string_lossy().to_string(),
        journal_id: cursor.journal_id,
        next_usn: cursor.next_usn,
    };

    let response = send_request(&request)?;

    match response {
        HelperResponse::Changes { events, new_cursor } => {
            let changes: Vec<FsChange> = events.into_iter().map(wire_to_change).collect();
            let new = UsnCursor {
                journal_id: new_cursor.journal_id,
                next_usn: new_cursor.next_usn,
            };
            Ok((changes, new))
        }
        HelperResponse::Error { code, message } => Err(FsIndexerError::HelperError {
            code: format!("{:?}", code),
            message,
        }),
        other => Err(FsIndexerError::HelperError {
            code: "UnexpectedResponse".to_string(),
            message: format!("expected Changes, got {:?}", std::mem::discriminant(&other)),
        }),
    }
}

/// Open the named pipe, send a request, and read the response.
fn send_request(request: &HelperRequest) -> FsIndexerResult<HelperResponse> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(PIPE_NAME)
        .map_err(|e| FsIndexerError::HelperUnavailable {
            reason: format!("cannot open pipe {}: {}", PIPE_NAME, e),
        })?;

    let mut writer = BufWriter::new(&file);
    framing::write_message(&mut writer, request).map_err(|e| FsIndexerError::HelperError {
        code: "WriteError".to_string(),
        message: format!("failed to write request: {}", e),
    })?;
    // Ensure the write is flushed before reading
    drop(writer);

    let mut reader = BufReader::new(&file);
    let response: HelperResponse =
        framing::read_message(&mut reader).map_err(|e| FsIndexerError::HelperError {
            code: "ReadError".to_string(),
            message: format!("failed to read response: {}", e),
        })?;

    Ok(response)
}

/// Convert a wire index entry to a domain IndexEntry.
fn wire_to_entry(w: WireIndexEntry) -> IndexEntry {
    let modified_at: DateTime<Utc> =
        DateTime::from_timestamp(w.modified_at_epoch, 0).unwrap_or_else(Utc::now);
    IndexEntry {
        fid: w.fid,
        parent_fid: w.parent_fid,
        name: OsString::from(&w.name),
        name_lossy: w.name,
        full_path: PathBuf::from(&w.full_path),
        size: w.size,
        allocated_size: w.allocated_size,
        is_dir: w.is_dir,
        modified_at,
        attributes: w.attributes,
    }
}

/// Convert a wire change event to a domain FsChange.
fn wire_to_change(w: WireChange) -> FsChange {
    match w {
        WireChange::Created(entry) => FsChange::Created(wire_to_entry(entry)),
        WireChange::Deleted { fid } => FsChange::Deleted { fid, path: None },
        WireChange::Moved {
            fid,
            new_parent_fid,
            new_name,
        } => FsChange::Moved {
            fid,
            new_parent_fid,
            new_name: OsString::from(new_name),
        },
        WireChange::Modified { fid, new_size } => FsChange::Modified { fid, new_size },
        WireChange::FullRescanNeeded { volume, reason } => FsChange::FullRescanNeeded {
            volume: PathBuf::from(volume),
            reason,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_not_available_without_helper() {
        // No helper running in test environment — pipe should not exist
        assert!(!pipe_available());
    }

    #[test]
    fn wire_to_entry_conversion() {
        let wire = WireIndexEntry {
            fid: 100,
            parent_fid: 5,
            name: "test.txt".to_string(),
            full_path: "C:\\test.txt".to_string(),
            size: 4096,
            allocated_size: 4096,
            is_dir: false,
            modified_at_epoch: 1_700_000_000,
            attributes: 0x20,
        };
        let entry = wire_to_entry(wire);
        assert_eq!(entry.fid, 100);
        assert_eq!(entry.parent_fid, 5);
        assert_eq!(entry.name_lossy, "test.txt");
        assert_eq!(entry.full_path, PathBuf::from("C:\\test.txt"));
        assert_eq!(entry.size, 4096);
        assert!(!entry.is_dir);
    }

    #[test]
    fn wire_to_change_all_variants() {
        let created = wire_to_change(WireChange::Created(WireIndexEntry {
            fid: 1,
            parent_fid: 0,
            name: "new.txt".to_string(),
            full_path: "C:\\new.txt".to_string(),
            size: 100,
            allocated_size: 4096,
            is_dir: false,
            modified_at_epoch: 1_700_000_000,
            attributes: 0,
        }));
        assert!(matches!(created, FsChange::Created(_)));

        let deleted = wire_to_change(WireChange::Deleted { fid: 2 });
        assert!(matches!(deleted, FsChange::Deleted { fid: 2, .. }));

        let moved = wire_to_change(WireChange::Moved {
            fid: 3,
            new_parent_fid: 10,
            new_name: "renamed.txt".to_string(),
        });
        assert!(matches!(moved, FsChange::Moved { fid: 3, .. }));

        let modified = wire_to_change(WireChange::Modified {
            fid: 4,
            new_size: 8192,
        });
        assert!(matches!(modified, FsChange::Modified { fid: 4, .. }));

        let rescan = wire_to_change(WireChange::FullRescanNeeded {
            volume: "C:\\".to_string(),
            reason: "wrapped".to_string(),
        });
        assert!(matches!(rescan, FsChange::FullRescanNeeded { .. }));
    }
}
