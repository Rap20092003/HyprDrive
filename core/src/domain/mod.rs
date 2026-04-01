//! Core domain models — the heart of HyprDrive
//!
//! Pure types with zero I/O and zero async.
//! These models define the "nouns" of the system.

pub mod enums;
pub mod events;
pub mod filter;
pub mod id;
pub mod path;
pub mod security;
pub mod sort;
pub mod sync;
pub mod tags;
pub mod transfer;
pub mod undo;
pub mod virtual_folder;

// Re-export top-level types for convenience.
pub use enums::{FileCategory, ObjectKind, StorageTier, VolumeKind};
pub use events::{ObjectIndexed, PipelineBatchComplete};
pub use filter::{FilterExpr, SqlParam};
pub use id::{DeviceId, LibraryId, LocationId, ObjectId, TagId, VirtualFolderId, VolumeId};
pub use path::HdPath;
pub use security::{CapabilityToken, RevocationList};
pub use sort::{SortDirection, SortField};
pub use sync::{SyncOperation, VectorClock};
pub use tags::Tag;
pub use transfer::{TransferCheckpoint, TransferRoute};
pub use undo::{UndoEntry, UndoStack};
pub use virtual_folder::VirtualFolder;
