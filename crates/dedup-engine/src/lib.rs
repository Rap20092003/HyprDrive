//! Multi-strategy duplicate file detection engine.
//!
//! Finds duplicate files using three complementary strategies:
//! 1. **Content hashing** (BLAKE3 progressive): Exact content duplicates via
//!    size bucketing → partial hash → full hash pipeline.
//! 2. **Fuzzy filename matching** (Jaro-Winkler): Renamed copies like
//!    "report (1).pdf" or "Copy of report.pdf".
//! 3. **Perceptual image hashing** (blockhash, optional): Visually similar
//!    images even when resized or recompressed.
//!
//! # Architecture
//!
//! Inspired by [dupeguru](https://github.com/arsenetar/dupeguru):
//! - Size bucketing eliminates impossible duplicates (different size = different content)
//! - Progressive hashing minimizes I/O (4KB partial → 64KB streaming → mmap for >512MB)
//! - Rayon parallelism for all hash computations
//! - Union-find grouping with transitive closure and reference selection
//!
//! # Feature flags
//!
//! - `perceptual` (default): Enables perceptual image hashing via `image_hasher`.

pub mod error;
pub mod fuzzy;
pub mod grouping;
pub mod hasher;
pub mod perceptual;
pub mod scanner;

use std::path::{Path, PathBuf};

// Re-export key types
pub use error::{DeduplicateError, DeduplicateResult};
pub use fuzzy::{find_similar_names, normalize_name, FuzzyMatch};
pub use grouping::{group_matches, DupeGroup, MatchKind, UnionFind};
pub use hasher::{full_hash, full_hash_mmap, mid_hash, partial_hash, should_mid_hash};
pub use perceptual::{is_image, PerceptualMatch};
pub use scanner::{ContentMatch, DupeReport, DuplicateScanner, ScanStrategy};

#[cfg(feature = "perceptual")]
pub use perceptual::find_similar_images;

/// A file entry for duplicate scanning.
///
/// The common input type for all dedup strategies. Can be constructed
/// directly or via [`FileEntry::from_path`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileEntry {
    /// Full path to the file.
    pub path: PathBuf,
    /// File size in bytes.
    pub size: u64,
    /// Filename (with extension).
    pub name: String,
    /// File extension (lowercase, without dot), if any.
    pub extension: Option<String>,
    /// Last modification time as Unix timestamp.
    pub modified_at: i64,
    /// Inode number (Unix) or file index (Windows), if available.
    pub inode: Option<u64>,
}

impl FileEntry {
    /// Create a new FileEntry with all fields specified.
    pub fn new(
        path: PathBuf,
        size: u64,
        name: String,
        extension: Option<String>,
        modified_at: i64,
        inode: Option<u64>,
    ) -> Self {
        Self {
            path,
            size,
            name,
            extension,
            modified_at,
            inode,
        }
    }

    /// Create a FileEntry by reading metadata from the filesystem.
    #[tracing::instrument(skip_all, fields(path = %path.as_ref().display()))]
    pub fn from_path(path: impl AsRef<Path>) -> DeduplicateResult<Self> {
        let path = path.as_ref();
        let metadata = std::fs::metadata(path)?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase());
        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        Ok(Self {
            path: path.to_path_buf(),
            size: metadata.len(),
            name,
            extension,
            modified_at,
            inode: None,
        })
    }

    /// Path depth (number of components).
    pub fn path_depth(&self) -> usize {
        self.path.components().count()
    }

    /// Whether the filename contains common copy patterns.
    ///
    /// Detects: "Copy of X", "X - Copy", "X (N)" for N=1..99, and Windows "X~1" style.
    pub fn has_copy_pattern(&self) -> bool {
        let lower = self.name.to_lowercase();
        if lower.contains("copy of ") || lower.contains(" - copy") || lower.contains(" copy.") {
            return true;
        }
        // Detect " (N)" where N is 1..99
        if let Some(start) = lower.rfind(" (") {
            if let Some(end) = lower[start..].find(')') {
                let inside = &lower[start + 2..start + end];
                if inside.len() <= 2 && inside.chars().all(|c| c.is_ascii_digit()) {
                    if let Ok(n) = inside.parse::<u32>() {
                        if (1..=99).contains(&n) {
                            return true;
                        }
                    }
                }
            }
        }
        // Windows short name collision: "filename~1.ext"
        if lower.contains('~') {
            let stem = lower.rsplit('.').next_back().unwrap_or(&lower);
            if stem.ends_with("~1") || stem.ends_with("~2") || stem.ends_with("~3") {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn file_entry_new_fields() {
        let entry = FileEntry::new(
            PathBuf::from("/test/file.txt"),
            100,
            "file.txt".to_string(),
            Some("txt".to_string()),
            1234567890,
            None,
        );
        assert_eq!(entry.size, 100);
        assert_eq!(entry.name, "file.txt");
        assert_eq!(entry.extension, Some("txt".to_string()));
    }

    #[test]
    fn file_entry_from_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"hello world").unwrap();
        f.flush().unwrap();
        drop(f);

        let entry = FileEntry::from_path(&path).unwrap();
        assert_eq!(entry.size, 11);
        assert_eq!(entry.name, "test.txt");
        assert_eq!(entry.extension, Some("txt".to_string()));
    }

    #[test]
    fn file_entry_from_path_nonexistent() {
        let result = FileEntry::from_path("/nonexistent/file.txt");
        assert!(result.is_err());
    }

    #[test]
    fn path_depth_calculation() {
        let entry = FileEntry::new(
            PathBuf::from("/home/user/photos/img.jpg"),
            100,
            "img.jpg".to_string(),
            Some("jpg".to_string()),
            0,
            None,
        );
        assert!(entry.path_depth() >= 4);
    }

    #[test]
    fn has_copy_pattern_detection() {
        let copy1 = FileEntry::new(
            PathBuf::from("/test/Copy of file.txt"),
            100,
            "Copy of file.txt".to_string(),
            Some("txt".to_string()),
            0,
            None,
        );
        assert!(copy1.has_copy_pattern());

        let copy2 = FileEntry::new(
            PathBuf::from("/test/file (1).txt"),
            100,
            "file (1).txt".to_string(),
            Some("txt".to_string()),
            0,
            None,
        );
        assert!(copy2.has_copy_pattern());

        let original = FileEntry::new(
            PathBuf::from("/test/file.txt"),
            100,
            "file.txt".to_string(),
            Some("txt".to_string()),
            0,
            None,
        );
        assert!(!original.has_copy_pattern());
    }
}
