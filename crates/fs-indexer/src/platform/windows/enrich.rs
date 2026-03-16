//! Size enrichment pass — query file sizes via Win32 API.
//!
//! Phase 2 of the two-phase scan. Takes topology entries and fills in
//! `size` and `allocated_size` using `GetFileInformationByHandleEx(FileStandardInfo)`.

use crate::error::FsIndexerResult;
use crate::types::IndexEntry;

/// Batch size for handle operations to avoid handle exhaustion.
const ENRICH_BATCH_SIZE: usize = 1000;

/// RAII wrapper for Win32 HANDLEs that ensures `CloseHandle` is called on drop.
struct SafeHandle(windows::Win32::Foundation::HANDLE);

impl Drop for SafeHandle {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: handle was opened by CreateFileW and is valid.
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// Enrich a slice of index entries with file sizes.
///
/// Opens each file with shared access, queries `FileStandardInfo` for
/// `EndOfFile` (logical size) and `AllocationSize` (on-disk), then closes
/// the handle. Processes entries in batches of [`ENRICH_BATCH_SIZE`].
///
/// Files that cannot be opened (access denied, locked) get `size = 0`
/// and a warning is logged per ADR-007.
#[allow(unsafe_code)]
#[tracing::instrument(skip(entries), fields(count = entries.len()))]
pub fn enrich_sizes(entries: &mut [IndexEntry]) -> FsIndexerResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FileStandardInfo, GetFileInformationByHandleEx, FILE_ATTRIBUTE_NORMAL,
        FILE_GENERIC_READ, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        FILE_STANDARD_INFO, OPEN_EXISTING,
    };

    let total = entries.len();
    let mut enriched = 0u64;
    let mut skipped = 0u64;

    for chunk in entries.chunks_mut(ENRICH_BATCH_SIZE) {
        for entry in chunk.iter_mut() {
            // Skip directories — they don't have meaningful sizes
            if entry.is_dir {
                continue;
            }

            let wide_path: Vec<u16> = entry
                .full_path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();

            // Open with maximum sharing to avoid blocking other processes
            // SAFETY: Calling Win32 API with a valid null-terminated wide string path.
            let handle_result = unsafe {
                CreateFileW(
                    windows::core::PCWSTR(wide_path.as_ptr()),
                    FILE_GENERIC_READ.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    None,
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    None,
                )
            };

            let handle = match handle_result {
                Ok(h) => SafeHandle(h),
                Err(e) => {
                    tracing::warn!(
                        path = %entry.full_path.display(),
                        error = %e,
                        "access denied during size enrichment, defaulting to size=0"
                    );
                    skipped += 1;
                    continue;
                }
            };

            // Query file standard info for sizes
            let mut file_info = FILE_STANDARD_INFO::default();
            // SAFETY: handle is valid (just opened above), buffer is correctly sized.
            let info_result = unsafe {
                GetFileInformationByHandleEx(
                    handle.0,
                    FileStandardInfo,
                    &mut file_info as *mut _ as *mut _,
                    std::mem::size_of::<FILE_STANDARD_INFO>() as u32,
                )
            };

            if info_result.is_ok() {
                entry.size = file_info.EndOfFile as u64;
                entry.allocated_size = file_info.AllocationSize as u64;
                enriched += 1;
            } else {
                tracing::warn!(
                    path = %entry.full_path.display(),
                    "GetFileInformationByHandleEx failed, defaulting to size=0"
                );
                skipped += 1;
            }
            // handle is automatically closed via SafeHandle::drop
        }
    }

    tracing::info!(total, enriched, skipped, "size enrichment complete");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::IndexEntry;
    use chrono::Utc;
    use std::ffi::OsString;
    use std::path::PathBuf;

    fn make_test_entry(path: &str) -> IndexEntry {
        IndexEntry {
            fid: 1,
            parent_fid: 0,
            name: OsString::from(path),
            name_lossy: path.to_string(),
            full_path: PathBuf::from(path),
            size: 0,
            allocated_size: 0,
            is_dir: false,
            modified_at: Utc::now(),
            attributes: 0,
        }
    }

    #[test]
    fn enrich_batch_size_reasonable() {
        assert_eq!(ENRICH_BATCH_SIZE, 1000);
    }

    #[test]
    fn enrich_skips_directories() {
        let mut entries = vec![IndexEntry {
            fid: 1,
            parent_fid: 0,
            name: OsString::from("Windows"),
            name_lossy: "Windows".to_string(),
            full_path: PathBuf::from("C:\\Windows"),
            size: 0,
            allocated_size: 0,
            is_dir: true,
            modified_at: Utc::now(),
            attributes: 0x10,
        }];
        let result = enrich_sizes(&mut entries);
        assert!(result.is_ok());
        // Directories should keep size=0 (skipped, not attempted)
        assert_eq!(entries[0].size, 0);
    }

    #[test]
    fn enrich_nonexistent_file_does_not_panic() {
        let mut entries = vec![make_test_entry(
            "C:\\nonexistent_path_hyprdrive_test_12345\\file.txt",
        )];
        let result = enrich_sizes(&mut entries);
        assert!(result.is_ok(), "should not fail on non-existent file");
        assert_eq!(entries[0].size, 0, "non-existent file should have size=0");
    }

    /// Requires a real file on disk. Run manually:
    /// `cargo test -p hyprdrive-fs-indexer -- --ignored enrich_known_file`
    #[test]
    #[ignore]
    fn enrich_known_file_has_size() {
        // Windows\System32\notepad.exe should always exist
        let mut entries = vec![make_test_entry("C:\\Windows\\System32\\notepad.exe")];
        let result = enrich_sizes(&mut entries);
        assert!(result.is_ok(), "enrich_sizes failed: {:?}", result);
        assert!(entries[0].size > 0, "notepad.exe should have size > 0");
        assert!(
            entries[0].allocated_size >= entries[0].size,
            "allocated_size ({}) should be >= size ({})",
            entries[0].allocated_size,
            entries[0].size
        );
    }

    /// Verify that non-existent files don't crash — they just get skipped.
    #[test]
    #[ignore]
    fn enrich_nonexistent_file_skipped() {
        let mut entries = vec![make_test_entry("C:\\this_file_does_not_exist_12345.txt")];
        let result = enrich_sizes(&mut entries);
        assert!(result.is_ok());
        assert_eq!(entries[0].size, 0, "non-existent file should have size=0");
    }
}
