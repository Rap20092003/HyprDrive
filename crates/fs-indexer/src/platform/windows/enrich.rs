//! Size enrichment pass — query file sizes via Win32 API.
//!
//! Phase 2 of the two-phase scan. Takes topology entries and fills in
//! `size` and `allocated_size` using `GetFileInformationByHandleEx(FileStandardInfo)`.

use crate::error::FsIndexerResult;
use crate::types::IndexEntry;

/// Statistics from the enrichment pass.
#[derive(Debug, Clone)]
pub struct EnrichStats {
    /// Number of entries successfully enriched.
    pub enriched: u64,
    /// Number of entries skipped (access denied, locked, etc.).
    pub skipped: u64,
    /// Subset of skipped: access denied (0x80070005).
    pub access_denied: u64,
    /// Subset of skipped: file/path not found (0x80070002, 0x80070003).
    pub not_found: u64,
    /// Subset of skipped: other errors.
    pub other_errors: u64,
}

/// Batch size for handle operations to avoid handle exhaustion.
const ENRICH_BATCH_SIZE: usize = 5000;

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
pub fn enrich_sizes(entries: &mut [IndexEntry]) -> FsIndexerResult<EnrichStats> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FileStandardInfo, GetFileInformationByHandleEx, FILE_ATTRIBUTE_NORMAL,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_STANDARD_INFO, OPEN_EXISTING,
    };
    // Use minimal access rights — FILE_READ_ATTRIBUTES (0x80) is enough
    // for GetFileInformationByHandleEx(FileStandardInfo) and succeeds on
    // many system files that deny FILE_GENERIC_READ.
    const FILE_READ_ATTRIBUTES: u32 = 0x0080;

    let total = entries.len();
    let mut enriched = 0u64;
    let mut skipped = 0u64;
    let mut access_denied = 0u64;
    let mut not_found = 0u64;
    let mut other_errors = 0u64;

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
                    FILE_READ_ATTRIBUTES,
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
                    let code = e.code().0 as u32;
                    match code {
                        0x80070005 => access_denied += 1,
                        0x80070002 | 0x80070003 => not_found += 1,
                        _ => other_errors += 1,
                    }
                    tracing::trace!(
                        path = %entry.full_path.display(),
                        error = %e,
                        "size enrichment failed, defaulting to size=0"
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
                entry.size = file_info.EndOfFile.max(0) as u64;
                entry.allocated_size = file_info.AllocationSize.max(0) as u64;
                enriched += 1;
            } else {
                other_errors += 1;
                tracing::trace!(
                    path = %entry.full_path.display(),
                    "GetFileInformationByHandleEx failed, defaulting to size=0"
                );
                skipped += 1;
            }
            // handle is automatically closed via SafeHandle::drop
        }
    }

    tracing::info!(
        total,
        enriched,
        skipped,
        access_denied,
        not_found,
        other_errors,
        "size enrichment complete"
    );

    Ok(EnrichStats {
        enriched,
        skipped,
        access_denied,
        not_found,
        other_errors,
    })
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
        assert_eq!(ENRICH_BATCH_SIZE, 5000);
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
