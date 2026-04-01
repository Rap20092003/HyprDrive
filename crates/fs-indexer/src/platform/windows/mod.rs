//! Windows filesystem indexer — MFT enumeration + USN journal delta.
//!
//! Two-phase scanning approach (validated by Phase -1 spike):
//! 1. **Topology pass**: MFT enumeration via `usn-journal-rs` → FRN tree (no sizes)
//! 2. **Enrichment pass**: `GetFileInformationByHandleEx(FileStandardInfo)` → sizes
//!
//! Falls back to `jwalk` for non-NTFS volumes (FAT32, exFAT).

pub mod detect;
pub mod enrich;
pub mod listener;
pub mod mft;
pub mod pipe;
pub mod scanner;
pub mod usn;
pub mod util;
