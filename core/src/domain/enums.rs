//! Domain enums — categories, kinds, and tiers
//!
//! Pure types with no I/O. Each enum is serde-friendly and Display-ready.

use serde::{Deserialize, Serialize};
use std::fmt;

/// What kind of filesystem object this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObjectKind {
    /// A regular file.
    File,
    /// A directory / folder.
    Directory,
    /// A symbolic link.
    Symlink,
}

impl fmt::Display for ObjectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File => write!(f, "file"),
            Self::Directory => write!(f, "directory"),
            Self::Symlink => write!(f, "symlink"),
        }
    }
}

/// Storage temperature tier for data classification.
/// Hot = frequently accessed, Cold = rarely accessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageTier {
    /// Frequently accessed — keep on fastest storage.
    Hot,
    /// Occasionally accessed — standard storage.
    Warm,
    /// Rarely accessed — archive/deep storage.
    Cold,
}

impl StorageTier {
    /// Numeric priority: Hot=2, Warm=1, Cold=0.
    fn priority(self) -> u8 {
        match self {
            Self::Hot => 2,
            Self::Warm => 1,
            Self::Cold => 0,
        }
    }
}

impl Ord for StorageTier {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority().cmp(&other.priority())
    }
}

impl PartialOrd for StorageTier {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Where a volume lives — local disk or cloud provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VolumeKind {
    /// Local/attached disk.
    Local,
    /// Amazon S3 or compatible.
    S3,
    /// Google Drive.
    GDrive,
    /// Dropbox.
    Dropbox,
    /// Microsoft OneDrive.
    OneDrive,
    /// Backblaze B2.
    B2,
    /// SFTP / SSH remote.
    Sftp,
}

/// High-level category inferred from a file's extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileCategory {
    /// Video files (mp4, mkv, avi, etc.)
    Video,
    /// Image files (jpg, png, webp, etc.)
    Image,
    /// Audio files (mp3, flac, wav, etc.)
    Audio,
    /// Documents (pdf, docx, txt, etc.)
    Document,
    /// Source code (rs, py, js, etc.)
    Code,
    /// Archives (zip, tar, gz, etc.)
    Archive,
    /// Executables (exe, msi, app, etc.)
    Executable,
    /// Font files (ttf, otf, woff, etc.)
    Font,
    /// Anything not matched above.
    Other,
}

impl FileCategory {
    /// Infer category from a file extension (case-insensitive).
    ///
    /// Returns `Other` if extension is unrecognized.
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_ascii_lowercase().as_str() {
            // Video
            "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "mpg" | "mpeg"
            | "3gp" | "m2ts" | "mts" => Self::Video,

            // Image
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "svg" | "ico" | "tiff" | "tif"
            | "heic" | "heif" | "avif" | "raw" | "cr2" | "nef" | "psd" => Self::Image,

            // Audio
            "mp3" | "flac" | "wav" | "aac" | "ogg" | "wma" | "m4a" | "opus" | "aiff" | "alac" => {
                Self::Audio
            }

            // Document
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "rtf" | "odt"
            | "ods" | "odp" | "csv" | "md" | "epub" => Self::Document,

            // Code
            "rs" | "py" | "js" | "ts" | "jsx" | "tsx" | "html" | "css" | "c" | "cpp" | "h"
            | "hpp" | "java" | "go" | "rb" | "php" | "swift" | "kt" | "scala" | "zig"
            | "toml" | "yaml" | "yml" | "json" | "xml" | "sh" | "bash" | "ps1" | "sql"
            | "graphql" | "proto" | "lua" | "dart" => Self::Code,

            // Archive
            "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "zst" | "lz4" | "lzma"
            | "cab" | "iso" | "dmg" => Self::Archive,

            // Executable
            "exe" | "msi" | "app" | "deb" | "rpm" | "appimage" | "snap" | "flatpak" | "dll"
            | "so" | "dylib" => Self::Executable,

            // Font
            "ttf" | "otf" | "woff" | "woff2" | "eot" => Self::Font,

            _ => Self::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_kind_serde_roundtrip() {
        for kind in [ObjectKind::File, ObjectKind::Directory, ObjectKind::Symlink] {
            let json = serde_json::to_string(&kind).ok().unwrap(); // test-only unwrap
            let back: ObjectKind = serde_json::from_str(&json).ok().unwrap();
            assert_eq!(kind, back);
        }
    }

    #[test]
    fn object_kind_display() {
        assert_eq!(ObjectKind::File.to_string(), "file");
        assert_eq!(ObjectKind::Directory.to_string(), "directory");
        assert_eq!(ObjectKind::Symlink.to_string(), "symlink");
    }

    #[test]
    fn storage_tier_ordering() {
        assert!(StorageTier::Hot > StorageTier::Warm);
        assert!(StorageTier::Warm > StorageTier::Cold);
        assert!(StorageTier::Hot > StorageTier::Cold);
    }

    #[test]
    fn storage_tier_serde_roundtrip() {
        for tier in [StorageTier::Hot, StorageTier::Warm, StorageTier::Cold] {
            let json = serde_json::to_string(&tier).ok().unwrap();
            let back: StorageTier = serde_json::from_str(&json).ok().unwrap();
            assert_eq!(tier, back);
        }
    }

    #[test]
    fn volume_kind_all_variants_serde() {
        let kinds = [
            VolumeKind::Local,
            VolumeKind::S3,
            VolumeKind::GDrive,
            VolumeKind::Dropbox,
            VolumeKind::OneDrive,
            VolumeKind::B2,
            VolumeKind::Sftp,
        ];
        for kind in kinds {
            let json = serde_json::to_string(&kind).ok().unwrap();
            let back: VolumeKind = serde_json::from_str(&json).ok().unwrap();
            assert_eq!(kind, back);
        }
    }

    #[test]
    fn file_category_from_extension_coverage() {
        // Video
        assert_eq!(FileCategory::from_extension("mp4"), FileCategory::Video);
        assert_eq!(FileCategory::from_extension("mkv"), FileCategory::Video);
        assert_eq!(FileCategory::from_extension("webm"), FileCategory::Video);

        // Image
        assert_eq!(FileCategory::from_extension("jpg"), FileCategory::Image);
        assert_eq!(FileCategory::from_extension("png"), FileCategory::Image);
        assert_eq!(FileCategory::from_extension("heic"), FileCategory::Image);

        // Audio
        assert_eq!(FileCategory::from_extension("mp3"), FileCategory::Audio);
        assert_eq!(FileCategory::from_extension("flac"), FileCategory::Audio);

        // Document
        assert_eq!(FileCategory::from_extension("pdf"), FileCategory::Document);
        assert_eq!(FileCategory::from_extension("docx"), FileCategory::Document);

        // Code
        assert_eq!(FileCategory::from_extension("rs"), FileCategory::Code);
        assert_eq!(FileCategory::from_extension("py"), FileCategory::Code);
        assert_eq!(FileCategory::from_extension("tsx"), FileCategory::Code);

        // Archive
        assert_eq!(FileCategory::from_extension("zip"), FileCategory::Archive);
        assert_eq!(FileCategory::from_extension("tar"), FileCategory::Archive);

        // Executable
        assert_eq!(FileCategory::from_extension("exe"), FileCategory::Executable);

        // Font
        assert_eq!(FileCategory::from_extension("ttf"), FileCategory::Font);
        assert_eq!(FileCategory::from_extension("woff2"), FileCategory::Font);
    }

    #[test]
    fn file_category_unknown_extension() {
        assert_eq!(FileCategory::from_extension("xyz"), FileCategory::Other);
        assert_eq!(FileCategory::from_extension(""), FileCategory::Other);
    }

    #[test]
    fn file_category_case_insensitive() {
        assert_eq!(FileCategory::from_extension("MP4"), FileCategory::Video);
        assert_eq!(FileCategory::from_extension("Jpg"), FileCategory::Image);
        assert_eq!(FileCategory::from_extension("PDF"), FileCategory::Document);
    }
}
