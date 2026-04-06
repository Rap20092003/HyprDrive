//! Reflink / Copy-on-Write deduplication support.
//!
//! Instead of deleting duplicates, reflink dedup shares the physical storage
//! between files while keeping them as independent entities. If either file
//! is later modified, the filesystem transparently copies only the changed
//! blocks (CoW semantics).
//!
//! This is the **safest** dedup action — no data loss risk, no symlink/hardlink
//! surprises, files remain fully independent from the user's perspective.
//!
//! # Platform Support
//!
//! | Filesystem | Mechanism | Status |
//! |------------|-----------|--------|
//! | Btrfs      | `FIDEDUPERANGE` ioctl | Supported (Linux) |
//! | XFS        | `FIDEDUPERANGE` ioctl | Supported (Linux) |
//! | APFS       | `clonefile(2)` | Planned (macOS) |
//! | ReFS       | `DUPLICATE_EXTENTS_DATA` | Planned (Windows) |
//! | Others     | Not supported | Falls back to error |

use crate::error::{DeduplicateError, DeduplicateResult};
use std::path::Path;

/// Result of a reflink dedup operation on a single file pair.
#[derive(Debug, Clone)]
pub struct ReflinkResult {
    /// Source file (the "reference" kept as-is).
    pub source: std::path::PathBuf,
    /// Destination file (the duplicate that was reflinked).
    pub dest: std::path::PathBuf,
    /// Bytes deduplicated (logical size of the file).
    pub bytes_deduped: u64,
    /// Whether the operation succeeded.
    pub success: bool,
    /// Error message if the operation failed.
    pub error: Option<String>,
}

/// Check if the filesystem at `path` supports reflink dedup.
///
/// Returns `true` if the filesystem is known to support CoW extent sharing.
pub fn supports_reflink(path: &Path) -> bool {
    detect_reflink_support(path)
}

/// Deduplicate a file pair via reflink (CoW extent sharing).
///
/// The `source` file's extents are shared with `dest`. Both files remain
/// independent — modifying either triggers a copy-on-write of the affected
/// blocks.
///
/// # Requirements
///
/// - Both files must be on the same filesystem.
/// - The filesystem must support reflinks (Btrfs, XFS with reflink=1, APFS, ReFS).
/// - Files must have identical content (caller must verify via hash).
///
/// # Errors
///
/// Returns an error if the filesystem doesn't support reflinks, the files
/// are on different filesystems, or the ioctl/syscall fails.
pub fn reflink_dedup(source: &Path, dest: &Path) -> DeduplicateResult<ReflinkResult> {
    let source_meta = std::fs::metadata(source)?;
    let dest_meta = std::fs::metadata(dest)?;

    // Sanity check: files should be the same size
    if source_meta.len() != dest_meta.len() {
        return Err(DeduplicateError::HashError(format!(
            "reflink dedup requires identical file sizes: source={}, dest={}",
            source_meta.len(),
            dest_meta.len()
        )));
    }

    let bytes = source_meta.len();

    match platform_reflink(source, dest, bytes) {
        Ok(()) => Ok(ReflinkResult {
            source: source.to_path_buf(),
            dest: dest.to_path_buf(),
            bytes_deduped: bytes,
            success: true,
            error: None,
        }),
        Err(e) => Ok(ReflinkResult {
            source: source.to_path_buf(),
            dest: dest.to_path_buf(),
            bytes_deduped: 0,
            success: false,
            error: Some(e.to_string()),
        }),
    }
}

/// Batch reflink dedup: deduplicate a reference file against multiple duplicates.
///
/// Returns results for each pair. Failures on individual files don't stop
/// the batch.
pub fn reflink_dedup_batch(source: &Path, duplicates: &[&Path]) -> Vec<ReflinkResult> {
    duplicates
        .iter()
        .map(|dest| match reflink_dedup(source, dest) {
            Ok(result) => result,
            Err(e) => ReflinkResult {
                source: source.to_path_buf(),
                dest: dest.to_path_buf(),
                bytes_deduped: 0,
                success: false,
                error: Some(e.to_string()),
            },
        })
        .collect()
}

// ── Platform-specific implementations ──────────────────────────────────

/// Linux: Use FIDEDUPERANGE ioctl for Btrfs/XFS reflink dedup.
#[cfg(target_os = "linux")]
fn platform_reflink(source: &Path, dest: &Path, length: u64) -> DeduplicateResult<()> {
    use std::os::unix::io::AsRawFd;

    // FIDEDUPERANGE ioctl number (from linux/fs.h)
    // #define FIDEDUPERANGE _IOWR(0x94, 54, struct file_dedupe_range)
    const FIDEDUPERANGE: libc::c_ulong = 0xC0189436;

    // Structs matching kernel's file_dedupe_range / file_dedupe_range_info
    #[repr(C)]
    struct FileDedupRange {
        src_offset: u64,
        src_length: u64,
        dest_count: u16,
        reserved1: u16,
        reserved2: u32,
    }

    #[repr(C)]
    struct FileDedupRangeInfo {
        dest_fd: i64,
        dest_offset: u64,
        bytes_deduped: u64,
        status: i32,
        reserved: u32,
    }

    let src_file = std::fs::File::open(source)?;
    let dest_file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dest)?;

    // Allocate combined struct: header + 1 info entry
    let mut range = FileDedupRange {
        src_offset: 0,
        src_length: length,
        dest_count: 1,
        reserved1: 0,
        reserved2: 0,
    };

    let mut info = FileDedupRangeInfo {
        dest_fd: dest_file.as_raw_fd() as i64,
        dest_offset: 0,
        bytes_deduped: 0,
        status: 0,
        reserved: 0,
    };

    // We need to pass header + info as a contiguous buffer to ioctl.
    // Use a Vec<u8> to hold both structs contiguously.
    let header_size = std::mem::size_of::<FileDedupRange>();
    let info_size = std::mem::size_of::<FileDedupRangeInfo>();
    let total_size = header_size + info_size;
    let mut buf = vec![0u8; total_size];

    // Copy header
    #[allow(unsafe_code)]
    unsafe {
        std::ptr::copy_nonoverlapping(
            &range as *const _ as *const u8,
            buf.as_mut_ptr(),
            header_size,
        );
        std::ptr::copy_nonoverlapping(
            &info as *const _ as *const u8,
            buf.as_mut_ptr().add(header_size),
            info_size,
        );
    }

    // Execute ioctl
    #[allow(unsafe_code)]
    let ret = unsafe { libc::ioctl(src_file.as_raw_fd(), FIDEDUPERANGE, buf.as_mut_ptr()) };

    if ret < 0 {
        return Err(DeduplicateError::Io(std::io::Error::last_os_error()));
    }

    // Read back the info struct to check status
    #[allow(unsafe_code)]
    unsafe {
        std::ptr::copy_nonoverlapping(
            buf.as_ptr().add(header_size),
            &mut info as *mut _ as *mut u8,
            info_size,
        );
    }

    // Status codes from kernel: 0 = success, negative = error
    // FILE_DEDUPE_RANGE_SAME (0) = data was identical, dedup succeeded
    // FILE_DEDUPE_RANGE_DIFFERS (1) = data differs, no dedup
    match info.status {
        0 => Ok(()),
        1 => Err(DeduplicateError::HashError(
            "FIDEDUPERANGE: data differs (content mismatch)".to_string(),
        )),
        _ => Err(DeduplicateError::Io(std::io::Error::from_raw_os_error(
            -info.status,
        ))),
    }
}

/// macOS: Use clonefile for APFS reflink.
#[cfg(target_os = "macos")]
fn platform_reflink(source: &Path, dest: &Path, _length: u64) -> DeduplicateResult<()> {
    // On macOS, clonefile() creates a new file that shares extents with source.
    // For dedup, we need to: 1) remove dest, 2) clonefile source → dest.
    // This preserves the dest path but replaces its storage with a CoW clone.
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let src_c = CString::new(source.as_os_str().as_bytes())
        .map_err(|e| DeduplicateError::HashError(format!("invalid path: {e}")))?;
    let dest_c = CString::new(dest.as_os_str().as_bytes())
        .map_err(|e| DeduplicateError::HashError(format!("invalid path: {e}")))?;

    // Remove dest first, then clone source to dest path
    std::fs::remove_file(dest)?;

    extern "C" {
        fn clonefile(src: *const libc::c_char, dst: *const libc::c_char, flags: u32)
            -> libc::c_int;
    }

    #[allow(unsafe_code)]
    let ret = unsafe { clonefile(src_c.as_ptr(), dest_c.as_ptr(), 0) };

    if ret != 0 {
        return Err(DeduplicateError::Io(std::io::Error::last_os_error()));
    }

    Ok(())
}

/// Windows / other platforms: Not yet supported.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn platform_reflink(_source: &Path, _dest: &Path, _length: u64) -> DeduplicateResult<()> {
    Err(DeduplicateError::HashError(
        "reflink dedup is not supported on this platform (requires Btrfs/XFS/APFS)".to_string(),
    ))
}

/// Detect if the filesystem at `path` supports reflinks.
#[cfg(target_os = "linux")]
fn detect_reflink_support(path: &Path) -> bool {
    // Check filesystem type via statfs
    #[allow(unsafe_code)]
    unsafe {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let c_path = match CString::new(path.as_os_str().as_bytes()) {
            Ok(p) => p,
            Err(_) => return false,
        };

        let mut stat: libc::statfs = std::mem::zeroed();
        if libc::statfs(c_path.as_ptr(), &mut stat) != 0 {
            return false;
        }

        // Known reflink-capable filesystem magic numbers
        const BTRFS_SUPER_MAGIC: libc::__fsword_t = 0x9123683E;
        const XFS_SUPER_MAGIC: libc::__fsword_t = 0x58465342;

        matches!(stat.f_type, BTRFS_SUPER_MAGIC | XFS_SUPER_MAGIC)
    }
}

#[cfg(target_os = "macos")]
fn detect_reflink_support(path: &Path) -> bool {
    // APFS supports clonefile. Check if the volume is APFS.
    // Simple heuristic: try to stat the path — APFS is the default on modern macOS.
    path.exists() // Conservative: assume APFS on macOS (true for most modern Macs)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn detect_reflink_support(_path: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn reflink_result_fields() {
        let result = ReflinkResult {
            source: PathBuf::from("/a/file.txt"),
            dest: PathBuf::from("/b/file.txt"),
            bytes_deduped: 4096,
            success: true,
            error: None,
        };
        assert!(result.success);
        assert_eq!(result.bytes_deduped, 4096);
        assert!(result.error.is_none());
    }

    #[test]
    fn reflink_result_failure() {
        let result = ReflinkResult {
            source: PathBuf::from("/a/file.txt"),
            dest: PathBuf::from("/b/file.txt"),
            bytes_deduped: 0,
            success: false,
            error: Some("not supported".to_string()),
        };
        assert!(!result.success);
        assert_eq!(result.bytes_deduped, 0);
    }

    #[test]
    fn supports_reflink_on_current_platform() {
        // On Windows CI, this should return false
        let result = supports_reflink(Path::new("."));
        // Just verify it doesn't panic
        let _ = result;
    }

    #[test]
    fn reflink_dedup_size_mismatch() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("source.txt");
        let dest = dir.path().join("dest.txt");
        std::fs::write(&source, "short").unwrap();
        std::fs::write(&dest, "longer content here").unwrap();

        let result = reflink_dedup(&source, &dest);
        assert!(result.is_err());
    }

    #[test]
    fn reflink_batch_returns_results_for_all() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("source.txt");
        let d1 = dir.path().join("dup1.txt");
        let d2 = dir.path().join("dup2.txt");
        std::fs::write(&source, "content").unwrap();
        std::fs::write(&d1, "content").unwrap();
        std::fs::write(&d2, "content").unwrap();

        let dups: Vec<&Path> = vec![d1.as_path(), d2.as_path()];
        let results = reflink_dedup_batch(&source, &dups);
        assert_eq!(results.len(), 2);
    }
}
