//! Object Pipeline — BLAKE3 hashing + database persistence for HyprDrive.
//!
//! This crate bridges raw `IndexEntry` results from `hyprdrive-fs-indexer`
//! to content-addressed objects in the database. The pipeline:
//!
//! 1. Checks the inode cache (redb) for previously-hashed entries
//! 2. Hashes cache misses with BLAKE3 via `hyprdrive-dedup-engine`
//! 3. Upserts `ObjectRow` + `LocationRow` into SQLite
//! 4. Emits `PipelineBatchComplete` events for observability
//!
//! # Example
//!
//! ```text
//! let config = PipelineConfig::new("volume-id".into());
//! let pipeline = ObjectPipeline::new(config, pool, cache);
//! let stats = pipeline.process_entries(&scan_result.entries).await?;
//! println!("Processed {} entries in {:?}", stats.total, stats.elapsed);
//! ```

#![allow(missing_docs)]

pub mod background_hasher;
pub mod change_processor;
pub mod error;
pub mod hasher;
pub mod pipeline;

pub use background_hasher::{run_background_hasher, BackgroundHashResult, BackgroundHasherConfig};
pub use change_processor::{ChangeProcessor, ChangeStats};
pub use error::{PipelineError, PipelineResult};
pub use hasher::hash_file;
pub use pipeline::{
    location_id_for_entry, mime_from_extension, ObjectPipeline, PipelineConfig, NO_PARENT_FID,
};
