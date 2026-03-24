//! Domain events for the object pipeline.
//!
//! Pure data types — no I/O, no async. These events are emitted by the
//! pipeline to signal progress and completion.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

use super::id::ObjectId;

/// Emitted when a single object has been indexed (hashed + inserted).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectIndexed {
    /// Content-addressed ID of the object.
    pub object_id: ObjectId,
    /// Deterministic location ID (hex string).
    pub location_id: String,
    /// Full filesystem path.
    pub path: PathBuf,
}

/// Emitted when a pipeline batch completes processing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineBatchComplete {
    /// Total entries in the batch.
    pub total: usize,
    /// Entries that were hashed (cache misses).
    pub hashed: usize,
    /// Entries served from inode cache (cache hits).
    pub cached: usize,
    /// Entries skipped (errors, permissions, etc.).
    pub skipped: usize,
    /// Number of individual file errors.
    pub errors: usize,
    /// Number of directory entries (synthetic ObjectIds).
    pub directories: usize,
    /// Number of zero-byte file entries.
    pub zero_byte: usize,
    /// Wall-clock time for the batch.
    #[serde(with = "duration_millis")]
    pub elapsed: Duration,
}

/// Serde helper: serialize Duration as milliseconds (u64).
mod duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let ms = u64::deserialize(d)?;
        Ok(Duration::from_millis(ms))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn object_indexed_serde_roundtrip() {
        let event = ObjectIndexed {
            object_id: ObjectId::from_blake3(b"test"),
            location_id: "abc123".to_string(),
            path: PathBuf::from("/test/file.txt"),
        };
        let json = serde_json::to_string(&event).expect("serialize");
        let back: ObjectIndexed = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.object_id, event.object_id);
        assert_eq!(back.location_id, event.location_id);
        assert_eq!(back.path, event.path);
    }

    #[test]
    fn pipeline_batch_complete_serde_roundtrip() {
        let event = PipelineBatchComplete {
            total: 1000,
            hashed: 50,
            cached: 940,
            skipped: 10,
            errors: 3,
            directories: 100,
            zero_byte: 5,
            elapsed: Duration::from_millis(1234),
        };
        let json = serde_json::to_string(&event).expect("serialize");
        let back: PipelineBatchComplete = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.total, 1000);
        assert_eq!(back.hashed, 50);
        assert_eq!(back.cached, 940);
        assert_eq!(back.directories, 100);
        assert_eq!(back.zero_byte, 5);
        assert_eq!(back.elapsed, Duration::from_millis(1234));
    }

    #[test]
    fn events_are_debug_and_clone() {
        let event = ObjectIndexed {
            object_id: ObjectId::from_blake3(b"hello"),
            location_id: "loc1".to_string(),
            path: PathBuf::from("/a/b"),
        };
        let _cloned = event.clone();
        let _debug = format!("{:?}", event);

        let batch = PipelineBatchComplete {
            total: 0,
            hashed: 0,
            cached: 0,
            skipped: 0,
            errors: 0,
            directories: 0,
            zero_byte: 0,
            elapsed: Duration::ZERO,
        };
        let _cloned = batch.clone();
        let _debug = format!("{:?}", batch);
    }
}
