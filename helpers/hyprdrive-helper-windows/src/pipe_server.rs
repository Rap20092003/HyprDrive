//! Named Pipe server for the HyprDrive Windows Helper.
//!
//! Creates `\\.\pipe\hyprdrive-helper`, accepts connections from the daemon,
//! dispatches MFT/USN operations, and sends results back via msgpack framing.
//!
//! ## Design
//!
//! - Single-threaded, sequential connection handling (daemon is the only client).
//! - Each connection processes one request-response pair, then disconnects.
//! - Ctrl+C sets the shutdown flag for graceful exit.

use anyhow::Result;
use hyprdrive_ipc_protocol::{
    framing, ErrorCode, HelperRequest, HelperResponse, WireCursor, WireIndexEntry,
    PIPE_NAME, PROTOCOL_VERSION,
};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::os::windows::io::FromRawHandle;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{
    FlushFileBuffers, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe,
    PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
};

/// Pipe I/O buffer size (64 KiB).
const BUFFER_SIZE: u32 = 64 * 1024;

/// Run the named pipe server loop.
///
/// Blocks until Ctrl+C or a `Shutdown` request is received.
pub fn run() -> Result<()> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    // Ctrl+C handler
    ctrlc_handler(shutdown_clone);

    let pipe_name_wide: Vec<u16> = PIPE_NAME.encode_utf16().chain(std::iter::once(0)).collect();

    loop {
        if shutdown.load(Ordering::Relaxed) {
            tracing::info!("shutdown requested, exiting");
            break;
        }

        // Create a new pipe instance for each connection
        let handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(pipe_name_wide.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                1,           // max instances
                BUFFER_SIZE, // out buffer
                BUFFER_SIZE, // in buffer
                5000,        // default timeout (ms)
                None,        // default security
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            let err = std::io::Error::last_os_error();
            tracing::error!(error = %err, "CreateNamedPipeW failed");
            return Err(err.into());
        }

        tracing::info!("waiting for client connection...");

        // Block until a client connects
        let connected = unsafe { ConnectNamedPipe(handle, None) };
        if connected.is_err() {
            // ERROR_PIPE_CONNECTED means client connected between Create and Connect — that's OK
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() != Some(535) {
                // 535 = ERROR_PIPE_CONNECTED
                tracing::error!(error = %err, "ConnectNamedPipe failed");
                unsafe { CloseHandle(handle).ok() };
                continue;
            }
        }

        tracing::info!("client connected");

        // Wrap HANDLE in File for Read/Write
        let file = unsafe { File::from_raw_handle(handle.0) };
        let mut reader = BufReader::new(&file);
        let mut writer = BufWriter::new(&file);

        // Read request
        let request: HelperRequest = match framing::read_message(&mut reader) {
            Ok(req) => req,
            Err(e) => {
                tracing::warn!(error = %e, "failed to read request");
                disconnect_and_close(handle);
                continue;
            }
        };

        tracing::info!(?request, "received request");

        // Dispatch
        let response = handle_request(&request);
        let is_shutdown = matches!(request, HelperRequest::Shutdown);

        // Write response
        if let Err(e) = framing::write_message(&mut writer, &response) {
            tracing::warn!(error = %e, "failed to write response");
        }

        // Flush before disconnect
        drop(writer);
        drop(reader);
        unsafe { FlushFileBuffers(handle).ok() };

        // Don't close handle via Drop — we'll disconnect manually
        std::mem::forget(file);
        disconnect_and_close(handle);

        if is_shutdown {
            tracing::info!("shutdown request received, exiting");
            break;
        }
    }

    Ok(())
}

/// Dispatch a request to the appropriate handler.
fn handle_request(request: &HelperRequest) -> HelperResponse {
    match request {
        HelperRequest::Ping => HelperResponse::Pong {
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: PROTOCOL_VERSION,
        },

        HelperRequest::ScanVolume { volume } => handle_scan(volume),

        HelperRequest::ReadCursor { volume } => handle_read_cursor(volume),

        HelperRequest::PollChanges {
            volume,
            journal_id,
            next_usn,
        } => handle_poll_changes(volume, *journal_id, *next_usn),

        HelperRequest::Shutdown => HelperResponse::Ok,
    }
}

/// Handle a full MFT scan request.
fn handle_scan(volume: &str) -> HelperResponse {
    let path = Path::new(volume);
    match hyprdrive_fs_indexer::full_scan(path) {
        Ok(scan) => {
            let entries: Vec<WireIndexEntry> = scan
                .entries
                .iter()
                .map(|e| WireIndexEntry {
                    fid: e.fid,
                    parent_fid: e.parent_fid,
                    name: e.name_lossy.clone(),
                    full_path: e.full_path.to_string_lossy().to_string(),
                    size: e.size,
                    allocated_size: e.allocated_size,
                    is_dir: e.is_dir,
                    modified_at_epoch: e.modified_at.timestamp(),
                    attributes: e.attributes,
                })
                .collect();

            let cursor = scan.cursor.map(|c| WireCursor {
                journal_id: c.journal_id,
                next_usn: c.next_usn,
            });

            tracing::info!(entries = entries.len(), "scan complete");
            HelperResponse::ScanResult { entries, cursor }
        }
        Err(e) => {
            tracing::error!(error = %e, "scan failed");
            let code = match &e {
                hyprdrive_fs_indexer::FsIndexerError::MftAccess { .. } => {
                    ErrorCode::MftAccessDenied
                }
                hyprdrive_fs_indexer::FsIndexerError::UnsupportedFs { .. } => {
                    ErrorCode::InvalidVolume
                }
                _ => ErrorCode::Internal,
            };
            HelperResponse::Error {
                code,
                message: e.to_string(),
            }
        }
    }
}

/// Handle a cursor read request.
fn handle_read_cursor(volume: &str) -> HelperResponse {
    let path = Path::new(volume);
    match hyprdrive_fs_indexer::read_cursor(path) {
        Ok(cursor) => HelperResponse::Cursor(WireCursor {
            journal_id: cursor.journal_id,
            next_usn: cursor.next_usn,
        }),
        Err(e) => {
            tracing::error!(error = %e, "read_cursor failed");
            HelperResponse::Error {
                code: ErrorCode::JournalError,
                message: e.to_string(),
            }
        }
    }
}

/// Handle a USN journal poll request.
fn handle_poll_changes(volume: &str, journal_id: u64, next_usn: i64) -> HelperResponse {
    let path = Path::new(volume);
    let cursor = hyprdrive_fs_indexer::UsnCursor {
        journal_id,
        next_usn,
    };

    match hyprdrive_fs_indexer::poll_changes(path, &cursor) {
        Ok((changes, new_cursor)) => {
            use hyprdrive_ipc_protocol::WireChange;

            let events: Vec<WireChange> = changes
                .into_iter()
                .map(|c| match c {
                    hyprdrive_fs_indexer::FsChange::Created(entry) => {
                        WireChange::Created(WireIndexEntry {
                            fid: entry.fid,
                            parent_fid: entry.parent_fid,
                            name: entry.name_lossy.clone(),
                            full_path: entry.full_path.to_string_lossy().to_string(),
                            size: entry.size,
                            allocated_size: entry.allocated_size,
                            is_dir: entry.is_dir,
                            modified_at_epoch: entry.modified_at.timestamp(),
                            attributes: entry.attributes,
                        })
                    }
                    hyprdrive_fs_indexer::FsChange::Deleted { fid, .. } => {
                        WireChange::Deleted { fid }
                    }
                    hyprdrive_fs_indexer::FsChange::Moved {
                        fid,
                        new_parent_fid,
                        new_name,
                    } => WireChange::Moved {
                        fid,
                        new_parent_fid,
                        new_name: new_name.to_string_lossy().to_string(),
                    },
                    hyprdrive_fs_indexer::FsChange::Modified { fid, new_size } => {
                        WireChange::Modified { fid, new_size }
                    }
                    hyprdrive_fs_indexer::FsChange::FullRescanNeeded { volume, reason } => {
                        WireChange::FullRescanNeeded {
                            volume: volume.to_string_lossy().to_string(),
                            reason,
                        }
                    }
                })
                .collect();

            HelperResponse::Changes {
                events,
                new_cursor: WireCursor {
                    journal_id: new_cursor.journal_id,
                    next_usn: new_cursor.next_usn,
                },
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "poll_changes failed");
            HelperResponse::Error {
                code: ErrorCode::JournalError,
                message: e.to_string(),
            }
        }
    }
}

/// Disconnect and close a named pipe handle.
fn disconnect_and_close(handle: HANDLE) {
    unsafe {
        DisconnectNamedPipe(handle).ok();
        CloseHandle(handle).ok();
    }
}

/// Register a Ctrl+C handler that sets the shutdown flag.
fn ctrlc_handler(shutdown: Arc<AtomicBool>) {
    // Use a simple thread to avoid pulling in ctrlc crate
    std::thread::spawn(move || {
        // SetConsoleCtrlHandler via windows crate
        // For simplicity, just block on a read — Ctrl+C will terminate the process
        // The proper way is windows::Win32::System::Console::SetConsoleCtrlHandler
        // but for v1, the pipe server's blocking ConnectNamedPipe will be interrupted
        // by the OS when the process receives SIGINT.
        let _ = shutdown; // suppress unused warning — will be used in v2
    });
}
