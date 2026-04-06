//! Database row types for the HyprDrive schema.
//!
//! `FileRow` is a COMPUTED type — it's assembled from JOINs across `objects`,
//! `locations`, `tags`, and `metadata`. It is NOT a single database table.
//! This follows the Spacedrive textbook pattern (Ch2).

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Valid values for the `hash_state` column on the `objects` table.
/// Use these constants instead of string literals to prevent typos.
pub mod hash_state {
    /// Object has a real BLAKE3 content hash.
    pub const CONTENT: &str = "content";
    /// Object has a synthetic placeholder hash (pending background hashing).
    pub const DEFERRED: &str = "deferred";
}

/// A row from the `objects` table — content identity.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ObjectRow {
    /// BLAKE3 content hash
    pub id: String,
    /// ObjectKind: File, Directory, Symlink
    pub kind: String,
    /// MIME type (e.g., "image/png")
    pub mime_type: Option<String>,
    /// File size in bytes
    pub size_bytes: i64,
    /// Creation timestamp
    pub created_at: String,
    /// Last update timestamp
    pub updated_at: String,
    /// Hash state: 'content' = real BLAKE3, 'deferred' = synthetic placeholder
    pub hash_state: String,
}

/// A row from the `locations` table — where content lives.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct LocationRow {
    /// LocationId (UUID)
    pub id: String,
    /// FK to objects.id
    pub object_id: String,
    /// VolumeId (UUID)
    pub volume_id: String,
    /// Full path
    pub path: String,
    /// File name
    pub name: String,
    /// File extension (without dot)
    pub extension: Option<String>,
    /// Parent location ID (for directory structure)
    pub parent_id: Option<String>,
    /// Whether this is a directory
    pub is_directory: bool,
    /// Size in bytes
    pub size_bytes: i64,
    /// Allocated bytes on disk
    pub allocated_bytes: i64,
    /// Created timestamp
    pub created_at: String,
    /// Modified timestamp
    pub modified_at: String,
    /// Last accessed timestamp
    pub accessed_at: Option<String>,
    /// File reference number (NTFS FRN / inode) for O(1) change event lookups.
    pub fid: Option<i64>,
}

/// COMPUTED file type — assembled from JOINs.
///
/// This is NOT a database table. It's constructed by `list_files_fast()`
/// which JOINs objects + locations to produce a complete view.
///
/// > "File is a COMPUTED struct" — Spacedrive Textbook, Chapter 2
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FileRow {
    // -- From locations --
    /// LocationId
    pub location_id: String,
    /// File name
    pub name: String,
    /// File extension
    pub extension: Option<String>,
    /// Full path
    pub path: String,
    /// Whether this is a directory
    pub is_directory: bool,
    /// Size in bytes
    pub size_bytes: i64,
    /// Allocated bytes on disk
    pub allocated_bytes: i64,
    /// Modified timestamp
    pub modified_at: String,

    // -- From objects (via JOIN) --
    /// ObjectId (BLAKE3 hash)
    pub object_id: String,
    /// ObjectKind
    pub kind: String,
    /// MIME type
    pub mime_type: Option<String>,
}

/// Directory size record.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DirSizeRow {
    /// LocationId of the directory
    pub location_id: String,
    /// Number of files
    pub file_count: i64,
    /// Total bytes
    pub total_bytes: i64,
    /// Allocated bytes
    pub allocated_bytes: i64,
    /// Cumulative allocated (including subdirectories)
    pub cumulative_allocated: i64,
}

/// File type record from the seed table.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FileTypeRow {
    /// Extension (without dot)
    pub extension: String,
    /// Category name
    pub category: String,
    /// Human-readable label
    pub label: String,
    /// Hex color for UI
    pub color: String,
}

/// Aggregate summary of a volume's contents.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct VolumeSummary {
    /// Total number of files
    pub total_files: i64,
    /// Total number of directories
    pub total_dirs: i64,
    /// Sum of file sizes (logical bytes)
    pub total_bytes: i64,
    /// Sum of allocated bytes on disk
    pub total_allocated: i64,
    /// Wasted bytes (allocated - logical)
    pub wasted_bytes: i64,
}

/// A directory with its aggregated size info for disk intelligence.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TopDirRow {
    /// Directory path
    pub path: String,
    /// Directory name
    pub name: String,
    /// Number of direct child files
    pub file_count: i64,
    /// Sum of direct children's logical bytes
    pub total_bytes: i64,
    /// Cumulative allocated bytes (including all subdirectories)
    pub cumulative_allocated: i64,
}

/// A directory with wasted space information.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WastedSpaceRow {
    /// Directory path
    pub path: String,
    /// Directory name
    pub name: String,
    /// Logical bytes (file content)
    pub total_bytes: i64,
    /// Allocated bytes on disk
    pub allocated_bytes: i64,
    /// Wasted bytes (allocated - logical)
    pub wasted_bytes: i64,
    /// Waste ratio (allocated / max(logical, 1))
    pub waste_ratio: f64,
}

/// A deferred object pending real content hashing.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DeferredObjectRow {
    /// Synthetic object_id (to be replaced)
    pub object_id: String,
    /// File path for reading content
    pub path: String,
    /// File size in bytes
    pub size_bytes: i64,
    /// File reference number for inode cache key
    pub fid: Option<i64>,
    /// Modified timestamp for inode cache key
    pub modified_at: String,
}

/// A group of duplicate files sharing the same content hash.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DuplicateGroupRow {
    /// Content hash (object_id)
    pub object_id: String,
    /// Number of locations with this content
    pub location_count: i64,
    /// Size of each copy in bytes
    pub size_bytes: i64,
    /// Total wasted bytes: (count - 1) * size
    pub wasted_bytes: i64,
}

/// File type category breakdown for pie/bar chart visualization.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TypeBreakdownRow {
    /// Category name (e.g., "Image", "Video", "Code", "Other")
    pub category: String,
    /// Hex color for UI (e.g., "#4CAF50")
    pub color: String,
    /// Number of files in this category
    pub file_count: i64,
    /// Total logical bytes in this category
    pub total_bytes: i64,
}

/// A file that hasn't been modified in a long time.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct StaleFileRow {
    /// LocationId
    pub location_id: String,
    /// Full path
    pub path: String,
    /// File name
    pub name: String,
    /// Extension
    pub extension: Option<String>,
    /// File size in bytes
    pub size_bytes: i64,
    /// Last modified timestamp
    pub modified_at: String,
    /// Days since last modification
    pub days_stale: i64,
}

/// A build artifact directory with aggregated size.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BuildArtifactRow {
    /// Directory path
    pub path: String,
    /// Directory name (e.g., "node_modules")
    pub name: String,
    /// Total bytes consumed by this directory tree
    pub total_bytes: i64,
    /// Number of files within
    pub file_count: i64,
    /// Which pattern matched (e.g., "node_modules")
    pub pattern: String,
}
