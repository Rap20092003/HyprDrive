//! Size enrichment pass — query file sizes via Win32 API.
//!
//! Phase 2 of the two-phase scan. Takes topology entries and fills in
//! `size` and `allocated_size` using `GetFileInformationByHandleEx(FileStandardInfo)`.

use crate::error::FsIndexerResult;
use crate::types::IndexEntry;

/// Batch size for handle operations to avoid handle exhaustion.
const ENRICH_BATCH_SIZE: usize = 1000;

/// Enrich a slice of index entries with file sizes.
///
/// Opens each file with shared access, queries `FileStandardInfo` for
/// `EndOfFile` (logical size) and `AllocationSize` (on-disk), then closes
/// the handle. Processes entries in batches of [`ENRICH_BATCH_SIZE`].
///
/// Files that cannot be opened (access denied, locked) get `size = 0`
/// and a warning is logged per ADR-007.
#[tracing::instrument(skip(entries), fields(count = entries.len()))]
pub fn enrich_sizes(entries: &mut [IndexEntry]) -> FsIndexerResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, GetFileInformationByHandleEx, FileStandardInfo,
        FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING, FILE_STANDARD_INFO,
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
                Ok(h) => h,
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
                    handle,
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

            // SAFETY: handle is valid and was opened by us.
            unsafe {
                let _ = CloseHandle(handle);
            }
        }
    }

    tracing::info!(
        total,
        enriched,
        skipped,
        "size enrichment complete"
    );

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
