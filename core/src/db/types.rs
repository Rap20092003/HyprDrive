//! Database row types for the HyprDrive schema.
//!
//! `FileRow` is a COMPUTED type — it's assembled from JOINs across `objects`,
//! `locations`, `tags`, and `metadata`. It is NOT a single database table.
//! This follows the Spacedrive textbook pattern (Ch2).

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

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
