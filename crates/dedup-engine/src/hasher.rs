//! Progressive BLAKE3 file hashing.
//!
//! Four tiers of hashing for efficient duplicate elimination:
//! 1. **Partial hash**: First 4KB only — cheap, eliminates most non-duplicates.
//! 2. **Mid hash**: First 1MB — catches header-identical files (ISO, DB, etc.).
//! 3. **Full hash**: Streaming 64KB chunks — confirms exact content match.
//! 4. **Full hash mmap**: Memory-mapped for files > 512MB — avoids heap allocation.

use crate::error::DeduplicateResult;
use std::cell::RefCell;
use std::io::Read;
use std::path::Path;

/// Size threshold for switching from streaming to mmap (512 MB).
const MMAP_THRESHOLD: u64 = 512 * 1024 * 1024;

/// Size of the partial hash read (4 KB).
const PARTIAL_HASH_SIZE: usize = 4096;

/// Size of the mid-hash read (1 MB).
///
/// Files with identical first 4KB but different content after that
/// (e.g. ISO images, database files with shared headers) are caught here,
/// avoiding the full-file read.
const MID_HASH_SIZE: usize = 1024 * 1024;

/// Size of streaming hash chunks (64 KB).
const CHUNK_SIZE: usize = 64 * 1024;

/// Minimum file size for the mid-hash stage to be useful.
///
/// Files smaller than this go straight from partial to full hash,
/// since mid-hash would read most of the file anyway.
const MID_HASH_MIN_FILE_SIZE: u64 = MID_HASH_SIZE as u64 * 2;

// Thread-local reusable buffers to avoid per-call heap allocation.
// Pattern borrowed from Czkawka's thread-local 2MB buffer approach.
thread_local! {
    static CHUNK_BUF: RefCell<Vec<u8>> = RefCell::new(vec![0u8; CHUNK_SIZE]);
    static MID_BUF: RefCell<Vec<u8>> = RefCell::new(vec![0u8; MID_HASH_SIZE]);
}

/// Compute a partial hash of a file (first 4KB).
///
/// Files smaller than 4KB are hashed in their entirety.
/// Two files with different partial hashes are guaranteed to be different.
#[tracing::instrument(skip_all, fields(path = %path.display()))]
pub fn partial_hash(path: &Path) -> DeduplicateResult<[u8; 32]> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = [0u8; PARTIAL_HASH_SIZE];
    let n = file.read(&mut buf)?;
    Ok(*blake3::hash(&buf[..n]).as_bytes())
}

/// Compute a mid-level hash of a file (first 1MB).
///
/// Catches files that share the same first 4KB header but diverge later.
/// Only useful for files >= 2MB; smaller files should skip to full hash.
#[tracing::instrument(skip_all, fields(path = %path.display()))]
pub fn mid_hash(path: &Path) -> DeduplicateResult<[u8; 32]> {
    MID_BUF.with(|buf| {
        let mut buf = buf.borrow_mut();
        let mut file = std::fs::File::open(path)?;
        let mut total = 0;
        while total < MID_HASH_SIZE {
            let n = file.read(&mut buf[total..])?;
            if n == 0 {
                break;
            }
            total += n;
        }
        Ok(*blake3::hash(&buf[..total]).as_bytes())
    })
}

/// Returns `true` if the file is large enough for the mid-hash stage to help.
pub fn should_mid_hash(file_size: u64) -> bool {
    file_size >= MID_HASH_MIN_FILE_SIZE
}

/// Compute a full BLAKE3 hash of a file using streaming 64KB chunks.
///
/// For files > 512MB, delegates to [`full_hash_mmap`] for better performance.
/// Uses a thread-local buffer to avoid per-call allocation.
#[tracing::instrument(skip_all, fields(path = %path.display()))]
pub fn full_hash(path: &Path) -> DeduplicateResult<[u8; 32]> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > MMAP_THRESHOLD {
        return full_hash_mmap(path);
    }

    CHUNK_BUF.with(|buf| {
        let mut buf = buf.borrow_mut();
        let mut file = std::fs::File::open(path)?;
        let mut hasher = blake3::Hasher::new();

        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }

        Ok(*hasher.finalize().as_bytes())
    })
}

/// Compute a full BLAKE3 hash using memory-mapped I/O.
///
/// Best for files > 512MB where streaming would be slow.
#[tracing::instrument(skip_all, fields(path = %path.display()))]
pub fn full_hash_mmap(path: &Path) -> DeduplicateResult<[u8; 32]> {
    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;

    if metadata.len() == 0 {
        return Ok(*blake3::hash(&[]).as_bytes());
    }

    // SAFETY: We only read the file. The file must not be modified externally
    // during hashing, which is acceptable for duplicate detection.
    #[allow(unsafe_code)]
    let mmap = unsafe { memmap2::Mmap::map(&file)? };
    Ok(*blake3::hash(&mmap).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_temp(content: &[u8]) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn partial_hash_deterministic() {
        let f = write_temp(b"hello world");
        let h1 = partial_hash(f.path()).unwrap();
        let h2 = partial_hash(f.path()).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn full_hash_deterministic() {
        let f = write_temp(b"hello world");
        let h1 = full_hash(f.path()).unwrap();
        let h2 = full_hash(f.path()).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn identical_files_same_hash() {
        let f1 = write_temp(b"identical content");
        let f2 = write_temp(b"identical content");
        assert_eq!(full_hash(f1.path()).unwrap(), full_hash(f2.path()).unwrap());
    }

    #[test]
    fn different_files_different_hash() {
        let f1 = write_temp(b"content A");
        let f2 = write_temp(b"content B");
        assert_ne!(full_hash(f1.path()).unwrap(), full_hash(f2.path()).unwrap());
    }

    #[test]
    fn partial_hash_identical_files() {
        let f1 = write_temp(b"same partial content");
        let f2 = write_temp(b"same partial content");
        assert_eq!(
            partial_hash(f1.path()).unwrap(),
            partial_hash(f2.path()).unwrap()
        );
    }

    #[test]
    fn empty_file_hashes_consistently() {
        let f1 = write_temp(b"");
        let f2 = write_temp(b"");
        assert_eq!(
            partial_hash(f1.path()).unwrap(),
            partial_hash(f2.path()).unwrap()
        );
        assert_eq!(full_hash(f1.path()).unwrap(), full_hash(f2.path()).unwrap());
    }

    #[test]
    fn small_file_partial_hash() {
        let f = write_temp(b"tiny");
        let h = partial_hash(f.path()).unwrap();
        assert_ne!(h, [0u8; 32]); // not all zeros
    }

    #[test]
    fn modified_file_different_hash() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.bin");

        std::fs::write(&path, b"original content").unwrap();
        let h1 = full_hash(&path).unwrap();

        std::fs::write(&path, b"modified content").unwrap();
        let h2 = full_hash(&path).unwrap();

        assert_ne!(h1, h2);
    }

    #[test]
    fn mmap_hash_matches_streaming() {
        let content = vec![42u8; 1024]; // 1KB
        let f = write_temp(&content);
        let streaming = full_hash(f.path()).unwrap();
        let mmap = full_hash_mmap(f.path()).unwrap();
        assert_eq!(streaming, mmap);
    }

    #[test]
    fn mid_hash_deterministic() {
        let f = write_temp(b"mid hash content");
        let h1 = mid_hash(f.path()).unwrap();
        let h2 = mid_hash(f.path()).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn mid_hash_identical_files() {
        let f1 = write_temp(b"same mid hash content");
        let f2 = write_temp(b"same mid hash content");
        assert_eq!(mid_hash(f1.path()).unwrap(), mid_hash(f2.path()).unwrap());
    }

    #[test]
    fn mid_hash_different_files() {
        let f1 = write_temp(b"content A for mid");
        let f2 = write_temp(b"content B for mid");
        assert_ne!(mid_hash(f1.path()).unwrap(), mid_hash(f2.path()).unwrap());
    }

    #[test]
    fn should_mid_hash_threshold() {
        // Files < 2MB should skip mid-hash
        assert!(!should_mid_hash(1024));
        assert!(!should_mid_hash(1024 * 1024)); // 1MB
                                                // Files >= 2MB benefit from mid-hash
        assert!(should_mid_hash(2 * 1024 * 1024));
        assert!(should_mid_hash(10 * 1024 * 1024));
    }

    #[test]
    fn small_file_mid_hash_matches_full_hash_of_same_data() {
        // For files < 1MB, mid_hash reads everything — same as hashing that content
        let content = vec![0xAB; 512]; // 512 bytes
        let f = write_temp(&content);
        let mh = mid_hash(f.path()).unwrap();
        // mid_hash of small file should equal blake3 of the whole content
        assert_eq!(mh, *blake3::hash(&content).as_bytes());
    }
}
