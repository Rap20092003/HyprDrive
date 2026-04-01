//! IPC wire protocol for HyprDrive helper communication.
//!
//! Serialized with MessagePack (`rmp-serde`) over Named Pipes (Windows)
//! or Unix domain sockets (future Linux/macOS).
//!
//! ## Wire Format
//!
//! Each message is length-prefixed:
//! ```text
//! [u32 LE length][msgpack payload]
//! ```
//!
//! The [`framing`] module provides encode/decode helpers used by both
//! the client (daemon) and server (helper binary).

use serde::{Deserialize, Serialize};

/// Named pipe path used by the Windows helper.
pub const PIPE_NAME: &str = r"\\.\pipe\hyprdrive-helper";

/// Maximum message size (128 MiB).
///
/// A full C:\ scan with 500k entries serializes to ~60-80 MiB in msgpack.
/// 128 MiB gives headroom; streaming/chunked responses are a future optimization.
pub const MAX_MESSAGE_SIZE: u32 = 128 * 1024 * 1024;

/// Protocol version for forward compatibility.
pub const PROTOCOL_VERSION: u32 = 1;

/// Request sent from daemon to helper.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HelperRequest {
    /// Ping — check if the helper is alive.
    Ping,

    /// Full MFT scan of a volume.
    ScanVolume {
        /// Volume root path, e.g. `"C:\\"`.
        volume: String,
    },

    /// Read the current USN journal cursor.
    ReadCursor {
        /// Volume root path.
        volume: String,
    },

    /// Poll USN journal for changes since a cursor position.
    PollChanges {
        /// Volume root path.
        volume: String,
        /// Journal ID from previous cursor.
        journal_id: u64,
        /// Next USN to read from.
        next_usn: i64,
    },

    /// Graceful shutdown request.
    Shutdown,
}

/// Response sent from helper to daemon.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HelperResponse {
    /// Pong — helper is alive.
    Pong {
        /// Helper binary version string.
        version: String,
        /// Protocol version supported by this helper.
        protocol_version: u32,
    },

    /// Full scan result.
    ScanResult {
        /// Serializable index entries.
        entries: Vec<WireIndexEntry>,
        /// USN cursor after scan (if available).
        cursor: Option<WireCursor>,
    },

    /// USN cursor position.
    Cursor(WireCursor),

    /// USN journal changes since last cursor.
    Changes {
        /// Change events.
        events: Vec<WireChange>,
        /// Updated cursor position.
        new_cursor: WireCursor,
    },

    /// Operation completed successfully (e.g., shutdown acknowledgement).
    Ok,

    /// Error response.
    Error {
        /// Error code for programmatic handling.
        code: ErrorCode,
        /// Human-readable error message.
        message: String,
    },
}

/// Wire-format index entry.
///
/// All fields are serde-friendly (no `OsString` or `PathBuf`).
/// The client converts these back to [`IndexEntry`](hyprdrive_fs_indexer::IndexEntry).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WireIndexEntry {
    /// File Reference Number (NTFS) or synthetic ID.
    pub fid: u64,
    /// Parent directory's FRN.
    pub parent_fid: u64,
    /// UTF-8 lossy filename.
    pub name: String,
    /// Full path from volume root.
    pub full_path: String,
    /// Logical file size in bytes.
    pub size: u64,
    /// On-disk allocated size in bytes.
    pub allocated_size: u64,
    /// Whether this entry is a directory.
    pub is_dir: bool,
    /// Last modification time as UTC epoch seconds.
    pub modified_at_epoch: i64,
    /// Win32 FILE_ATTRIBUTE_* flags.
    pub attributes: u32,
}

/// Wire-format USN cursor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WireCursor {
    /// The USN journal ID.
    pub journal_id: u64,
    /// The next USN to read from.
    pub next_usn: i64,
}

/// Wire-format filesystem change event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WireChange {
    /// A new file or directory was created.
    Created(WireIndexEntry),
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
        new_name: String,
    },
    /// A file's content was modified.
    Modified {
        /// FRN of the modified entry.
        fid: u64,
        /// New logical size after modification.
        new_size: u64,
    },
    /// A full rescan is needed.
    FullRescanNeeded {
        /// Volume path that needs rescanning.
        volume: String,
        /// Human-readable reason for the rescan.
        reason: String,
    },
}

/// Error codes for programmatic error handling.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ErrorCode {
    /// Volume not found or not NTFS.
    InvalidVolume,
    /// MFT access failed (permissions, volume locked, etc.).
    MftAccessDenied,
    /// USN journal error (wrapped, recreated, etc.).
    JournalError,
    /// Protocol version mismatch.
    ProtocolMismatch,
    /// Internal/unexpected error.
    Internal,
}

/// Length-prefixed message framing.
///
/// Wire format: `[u32 LE length][msgpack payload]`
///
/// Used by both client and server.
pub mod framing {
    use super::MAX_MESSAGE_SIZE;
    use serde::{de::DeserializeOwned, Serialize};
    use std::io::{self, Read, Write};

    /// Write a length-prefixed msgpack message to `writer`.
    ///
    /// # Errors
    ///
    /// Returns an error if the serialized message exceeds [`MAX_MESSAGE_SIZE`]
    /// or if a write/flush fails.
    pub fn write_message<W: Write, T: Serialize>(writer: &mut W, msg: &T) -> io::Result<()> {
        let payload =
            rmp_serde::to_vec(msg).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let len = payload.len() as u32;
        if len > MAX_MESSAGE_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("message too large: {len} > {MAX_MESSAGE_SIZE}"),
            ));
        }
        writer.write_all(&len.to_le_bytes())?;
        writer.write_all(&payload)?;
        writer.flush()
    }

    /// Read a length-prefixed msgpack message from `reader`.
    ///
    /// # Errors
    ///
    /// Returns an error if the length prefix exceeds [`MAX_MESSAGE_SIZE`],
    /// the read fails, or deserialization fails.
    pub fn read_message<R: Read, T: DeserializeOwned>(reader: &mut R) -> io::Result<T> {
        let mut len_buf = [0u8; 4];
        reader.read_exact(&mut len_buf)?;
        let len = u32::from_le_bytes(len_buf);
        if len > MAX_MESSAGE_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("message too large: {len} > {MAX_MESSAGE_SIZE}"),
            ));
        }
        let mut payload = vec![0u8; len as usize];
        reader.read_exact(&mut payload)?;
        rmp_serde::from_slice(&payload).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn request_roundtrip_ping() {
        let msg = HelperRequest::Ping;
        let mut buf = Vec::new();
        framing::write_message(&mut buf, &msg).unwrap();
        let back: HelperRequest = framing::read_message(&mut Cursor::new(&buf)).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn request_roundtrip_scan_volume() {
        let msg = HelperRequest::ScanVolume {
            volume: "C:\\".to_string(),
        };
        let mut buf = Vec::new();
        framing::write_message(&mut buf, &msg).unwrap();
        let back: HelperRequest = framing::read_message(&mut Cursor::new(&buf)).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn request_roundtrip_poll_changes() {
        let msg = HelperRequest::PollChanges {
            volume: "D:\\".to_string(),
            journal_id: 42,
            next_usn: 9999,
        };
        let mut buf = Vec::new();
        framing::write_message(&mut buf, &msg).unwrap();
        let back: HelperRequest = framing::read_message(&mut Cursor::new(&buf)).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn response_roundtrip_pong() {
        let msg = HelperResponse::Pong {
            version: "0.1.0".to_string(),
            protocol_version: PROTOCOL_VERSION,
        };
        let mut buf = Vec::new();
        framing::write_message(&mut buf, &msg).unwrap();
        let back: HelperResponse = framing::read_message(&mut Cursor::new(&buf)).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn response_roundtrip_scan_result() {
        let msg = HelperResponse::ScanResult {
            entries: vec![WireIndexEntry {
                fid: 100,
                parent_fid: 5,
                name: "test.txt".to_string(),
                full_path: "C:\\test.txt".to_string(),
                size: 4096,
                allocated_size: 4096,
                is_dir: false,
                modified_at_epoch: 1_700_000_000,
                attributes: 0x20,
            }],
            cursor: Some(WireCursor {
                journal_id: 1,
                next_usn: 500,
            }),
        };
        let mut buf = Vec::new();
        framing::write_message(&mut buf, &msg).unwrap();
        let back: HelperResponse = framing::read_message(&mut Cursor::new(&buf)).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn response_roundtrip_error() {
        let msg = HelperResponse::Error {
            code: ErrorCode::MftAccessDenied,
            message: "not admin".to_string(),
        };
        let mut buf = Vec::new();
        framing::write_message(&mut buf, &msg).unwrap();
        let back: HelperResponse = framing::read_message(&mut Cursor::new(&buf)).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn response_roundtrip_changes() {
        let msg = HelperResponse::Changes {
            events: vec![
                WireChange::Created(WireIndexEntry {
                    fid: 200,
                    parent_fid: 5,
                    name: "new.txt".to_string(),
                    full_path: "C:\\new.txt".to_string(),
                    size: 100,
                    allocated_size: 4096,
                    is_dir: false,
                    modified_at_epoch: 1_700_000_001,
                    attributes: 0x20,
                }),
                WireChange::Deleted { fid: 300 },
                WireChange::Moved {
                    fid: 400,
                    new_parent_fid: 10,
                    new_name: "renamed.txt".to_string(),
                },
                WireChange::Modified {
                    fid: 500,
                    new_size: 8192,
                },
                WireChange::FullRescanNeeded {
                    volume: "C:\\".to_string(),
                    reason: "journal wrapped".to_string(),
                },
            ],
            new_cursor: WireCursor {
                journal_id: 1,
                next_usn: 1000,
            },
        };
        let mut buf = Vec::new();
        framing::write_message(&mut buf, &msg).unwrap();
        let back: HelperResponse = framing::read_message(&mut Cursor::new(&buf)).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn framing_rejects_truncated_length() {
        let buf = [0u8; 2]; // only 2 bytes, need 4
        let result: Result<HelperRequest, _> = framing::read_message(&mut Cursor::new(&buf));
        assert!(result.is_err());
    }

    #[test]
    fn framing_rejects_truncated_payload() {
        let mut buf = Vec::new();
        // Write length prefix claiming 100 bytes
        buf.extend_from_slice(&100u32.to_le_bytes());
        // But only provide 10 bytes of payload
        buf.extend_from_slice(&[0u8; 10]);
        let result: Result<HelperRequest, _> = framing::read_message(&mut Cursor::new(&buf));
        assert!(result.is_err());
    }

    #[test]
    fn error_code_serde_roundtrip() {
        let codes = [
            ErrorCode::InvalidVolume,
            ErrorCode::MftAccessDenied,
            ErrorCode::JournalError,
            ErrorCode::ProtocolMismatch,
            ErrorCode::Internal,
        ];
        for code in codes {
            let json = serde_json::to_string(&code).unwrap();
            let back: ErrorCode = serde_json::from_str(&json).unwrap();
            assert_eq!(code, back);
        }
    }

    #[test]
    fn wire_cursor_serde_roundtrip() {
        let cursor = WireCursor {
            journal_id: 42,
            next_usn: 1000,
        };
        let mut buf = Vec::new();
        framing::write_message(&mut buf, &cursor).unwrap();
        let back: WireCursor = framing::read_message(&mut Cursor::new(&buf)).unwrap();
        assert_eq!(cursor, back);
    }

    #[test]
    fn request_roundtrip_shutdown() {
        let msg = HelperRequest::Shutdown;
        let mut buf = Vec::new();
        framing::write_message(&mut buf, &msg).unwrap();
        let back: HelperRequest = framing::read_message(&mut Cursor::new(&buf)).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn response_roundtrip_ok() {
        let msg = HelperResponse::Ok;
        let mut buf = Vec::new();
        framing::write_message(&mut buf, &msg).unwrap();
        let back: HelperResponse = framing::read_message(&mut Cursor::new(&buf)).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn multiple_messages_in_stream() {
        let mut buf = Vec::new();
        let msg1 = HelperRequest::Ping;
        let msg2 = HelperRequest::ScanVolume {
            volume: "D:\\".to_string(),
        };
        let msg3 = HelperRequest::Shutdown;
        framing::write_message(&mut buf, &msg1).unwrap();
        framing::write_message(&mut buf, &msg2).unwrap();
        framing::write_message(&mut buf, &msg3).unwrap();

        let mut cursor = Cursor::new(&buf);
        let back1: HelperRequest = framing::read_message(&mut cursor).unwrap();
        let back2: HelperRequest = framing::read_message(&mut cursor).unwrap();
        let back3: HelperRequest = framing::read_message(&mut cursor).unwrap();
        assert_eq!(msg1, back1);
        assert_eq!(msg2, back2);
        assert_eq!(msg3, back3);
    }
}
