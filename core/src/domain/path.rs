//! HyprDrive path abstraction — platform-agnostic resource addressing.
//!
//! `HdPath` is how HyprDrive refers to "where something lives." A file
//! can be on a local disk, in a cloud bucket, addressed by content hash,
//! or stored as a sidecar metadata blob alongside another object.
//!
//! This enum replaces raw `PathBuf` in domain-level APIs, enabling the
//! system to treat local files and cloud objects with a unified interface.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// A HyprDrive resource path — platform-agnostic, serde-friendly.
///
/// Every indexed resource in HyprDrive has an `HdPath` that describes
/// where it physically or logically lives. The daemon resolves these
/// into concrete I/O operations based on the variant.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum HdPath {
    /// A file or directory on a local/attached disk.
    ///
    /// Examples:
    /// - `Physical("C:\\Users\\alice\\photo.jpg")`
    /// - `Physical("/home/alice/photo.jpg")`
    Physical(PathBuf),

    /// A resource in a cloud storage provider.
    ///
    /// The `provider` field maps to [`VolumeKind`](super::enums::VolumeKind)
    /// (S3, GDrive, Dropbox, etc.). The `bucket` is provider-specific
    /// (S3 bucket name, Drive folder ID, etc.). The `key` is the object path.
    Cloud {
        /// Cloud provider identifier (e.g., "s3", "gdrive", "dropbox").
        provider: String,
        /// Provider-specific container (S3 bucket, Drive folder, etc.).
        bucket: String,
        /// Object key / path within the container.
        key: String,
    },

    /// A content-addressed reference (BLAKE3 hash).
    ///
    /// Used when the caller knows the content hash but not (or doesn't care
    /// about) the physical location. The daemon resolves this to a concrete
    /// location via the objects/locations tables.
    Content {
        /// Hex-encoded BLAKE3 hash (64 characters).
        blake3_hex: String,
    },

    /// Sidecar metadata stored alongside another object.
    ///
    /// Used for thumbnails, EXIF blobs, extracted text, etc.
    /// The `parent` is the HdPath of the primary object, and `suffix`
    /// identifies the sidecar type (e.g., ".thumb.webp", ".exif.json").
    Sidecar {
        /// The primary object this sidecar is attached to.
        parent: Box<HdPath>,
        /// Sidecar type suffix (e.g., ".thumb.webp", ".meta.json").
        suffix: String,
    },
}

impl HdPath {
    /// Create a physical path from anything that converts to `PathBuf`.
    pub fn physical(path: impl Into<PathBuf>) -> Self {
        Self::Physical(path.into())
    }

    /// Create a cloud path.
    pub fn cloud(
        provider: impl Into<String>,
        bucket: impl Into<String>,
        key: impl Into<String>,
    ) -> Self {
        Self::Cloud {
            provider: provider.into(),
            bucket: bucket.into(),
            key: key.into(),
        }
    }

    /// Create a content-addressed path from a hex hash string.
    pub fn content(blake3_hex: impl Into<String>) -> Self {
        Self::Content {
            blake3_hex: blake3_hex.into(),
        }
    }

    /// Create a sidecar path attached to a parent.
    pub fn sidecar(parent: HdPath, suffix: impl Into<String>) -> Self {
        Self::Sidecar {
            parent: Box::new(parent),
            suffix: suffix.into(),
        }
    }

    /// Returns `true` if this is a local physical path.
    pub fn is_physical(&self) -> bool {
        matches!(self, Self::Physical(_))
    }

    /// Returns `true` if this is a cloud path.
    pub fn is_cloud(&self) -> bool {
        matches!(self, Self::Cloud { .. })
    }

    /// Returns `true` if this is a content-addressed path.
    pub fn is_content(&self) -> bool {
        matches!(self, Self::Content { .. })
    }

    /// Returns `true` if this is a sidecar path.
    pub fn is_sidecar(&self) -> bool {
        matches!(self, Self::Sidecar { .. })
    }

    /// Extract the physical `PathBuf` if this is a `Physical` variant.
    pub fn as_physical(&self) -> Option<&PathBuf> {
        match self {
            Self::Physical(p) => Some(p),
            _ => None,
        }
    }
}

impl fmt::Display for HdPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Physical(p) => write!(f, "file://{}", p.display()),
            Self::Cloud {
                provider,
                bucket,
                key,
            } => {
                write!(f, "{}://{}/{}", provider, bucket, key)
            }
            Self::Content { blake3_hex } => write!(f, "blake3://{}", blake3_hex),
            Self::Sidecar { parent, suffix } => write!(f, "{}@sidecar:{}", parent, suffix),
        }
    }
}

impl From<PathBuf> for HdPath {
    fn from(path: PathBuf) -> Self {
        Self::Physical(path)
    }
}

impl From<&str> for HdPath {
    fn from(s: &str) -> Self {
        Self::Physical(PathBuf::from(s))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn physical_path_roundtrip() {
        let path = HdPath::physical("/home/alice/photo.jpg");
        assert!(path.is_physical());
        assert!(!path.is_cloud());
        assert_eq!(
            path.as_physical().unwrap(),
            &PathBuf::from("/home/alice/photo.jpg")
        );
    }

    #[test]
    fn cloud_path_display() {
        let path = HdPath::cloud("s3", "my-bucket", "photos/2024/img.jpg");
        assert!(path.is_cloud());
        assert_eq!(path.to_string(), "s3://my-bucket/photos/2024/img.jpg");
    }

    #[test]
    fn content_path_display() {
        let hex = "a".repeat(64);
        let path = HdPath::content(&hex);
        assert!(path.is_content());
        assert_eq!(path.to_string(), format!("blake3://{}", hex));
    }

    #[test]
    fn sidecar_path_display() {
        let parent = HdPath::physical("/home/alice/photo.jpg");
        let sidecar = HdPath::sidecar(parent, ".thumb.webp");
        assert!(sidecar.is_sidecar());
        assert_eq!(
            sidecar.to_string(),
            "file:///home/alice/photo.jpg@sidecar:.thumb.webp"
        );
    }

    #[test]
    fn serde_roundtrip_all_variants() {
        let paths = vec![
            HdPath::physical("C:\\test.txt"),
            HdPath::cloud("gdrive", "root", "docs/file.pdf"),
            HdPath::content("b".repeat(64)),
            HdPath::sidecar(HdPath::physical("/tmp/img.png"), ".exif.json"),
        ];
        for path in paths {
            let json = serde_json::to_string(&path).unwrap();
            let back: HdPath = serde_json::from_str(&json).unwrap();
            assert_eq!(path, back, "roundtrip failed for: {}", json);
        }
    }

    #[test]
    fn from_pathbuf_conversion() {
        let path: HdPath = PathBuf::from("/tmp/file.txt").into();
        assert!(path.is_physical());
    }

    #[test]
    fn from_str_conversion() {
        let path: HdPath = "/tmp/file.txt".into();
        assert!(path.is_physical());
    }

    #[test]
    fn physical_returns_none_for_non_physical() {
        let path = HdPath::content("a".repeat(64));
        assert!(path.as_physical().is_none());
    }

    #[test]
    fn equality_and_hash() {
        use std::collections::HashSet;
        let a = HdPath::physical("/test");
        let b = HdPath::physical("/test");
        let c = HdPath::physical("/other");
        assert_eq!(a, b);
        assert_ne!(a, c);

        let mut set = HashSet::new();
        set.insert(a.clone());
        assert!(set.contains(&b));
        assert!(!set.contains(&c));
    }
}
