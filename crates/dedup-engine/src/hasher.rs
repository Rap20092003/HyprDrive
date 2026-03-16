//! Progressive BLAKE3 file hashing.
//!
//! Three tiers of hashing for efficient duplicate elimination:
//! 1. **Partial hash**: First 4KB only — cheap, eliminates most non-duplicates.
//! 2. **Full hash**: Streaming 64KB chunks — confirms exact content match.
//! 3. **Full hash mmap**: Memory-mapped for files > 512MB — avoids heap allocation.

use crate::error::DeduplicateResult;
use std::io::Read;
use std::path::Path;

/// Size threshold for switching from streaming to mmap (512 MB).
const MMAP_THRESHOLD: u64 = 512 * 1024 * 1024;

/// Size of the partial hash read (4 KB).
const PARTIAL_HASH_SIZE: usize = 4096;

/// Size of streaming hash chunks (64 KB).
const CHUNK_SIZE: usize = 64 * 1024;

/// Compute a partial hash of a file (first 4KB).
///
/// Files smaller than 4KB are hashed in their entirety.
/// Two files with different partial hashes are guaranteed to be different.
#[tracing::instrument(skip_all, fields(path = %path.display()))]
pub fn partial_hash(path: &Path) -> DeduplicateResult<[u8; 32]> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; PARTIAL_HASH_SIZE];
    let n = file.read(&mut buf)?;
    buf.truncate(n);
    Ok(*blake3::hash(&buf).as_bytes())
}

/// Compute a full BLAKE3 hash of a file using streaming 64KB chunks.
///
/// For files > 512MB, delegates to [`full_hash_mmap`] for better performance.
#[tracing::instrument(skip_all, fields(path = %path.display()))]
pub fn full_hash(path: &Path) -> DeduplicateResult<[u8; 32]> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > MMAP_THRESHOLD {
        return full_hash_mmap(path);
    }

    let mut file = std::fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; CHUNK_SIZE];

    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(*hasher.finalize().as_bytes())
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
}
