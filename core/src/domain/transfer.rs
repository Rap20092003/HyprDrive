//! Transfer types — file transfer routing and checkpointing.

use roaring::RoaringBitmap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// How a file transfer is routed between devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferRoute {
    /// Direct LAN transfer (fastest).
    Lan,
    /// WAN transfer via public IPs.
    Wan,
    /// Relay transfer through a rendezvous server (fallback).
    Relay,
}

/// Checkpoint for resumable file transfers.
///
/// Tracks which chunks have been successfully transferred using a
/// `RoaringBitmap` — O(1) check, compact serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferCheckpoint {
    /// Unique transfer identifier.
    pub transfer_id: Uuid,
    /// Bitmap of completed chunk indices.
    pub completed_chunks: RoaringBitmap,
    /// Total number of chunks in this transfer.
    pub total_chunks: u32,
}

impl TransferCheckpoint {
    /// Create a new empty checkpoint.
    pub fn new(transfer_id: Uuid, total_chunks: u32) -> Self {
        Self {
            transfer_id,
            completed_chunks: RoaringBitmap::new(),
            total_chunks,
        }
    }

    /// Mark a chunk as completed.
    pub fn mark_complete(&mut self, chunk_index: u32) {
        self.completed_chunks.insert(chunk_index);
    }

    /// Check if a specific chunk has been completed.
    pub fn is_complete(&self, chunk_index: u32) -> bool {
        self.completed_chunks.contains(chunk_index)
    }

    /// Number of remaining chunks.
    pub fn remaining(&self) -> u32 {
        self.total_chunks
            .saturating_sub(self.completed_chunks.len() as u32)
    }

    /// Whether the entire transfer is complete.
    pub fn is_finished(&self) -> bool {
        self.completed_chunks.len() as u32 >= self.total_chunks
    }

    /// Get missing chunk indices.
    pub fn missing_chunks(&self) -> Vec<u32> {
        (0..self.total_chunks)
            .filter(|i| !self.completed_chunks.contains(*i))
            .collect()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn transfer_route_serde() {
        for route in [TransferRoute::Lan, TransferRoute::Wan, TransferRoute::Relay] {
            let json = serde_json::to_string(&route).ok().unwrap();
            let back: TransferRoute = serde_json::from_str(&json).ok().unwrap();
            assert_eq!(route, back);
        }
    }

    #[test]
    fn checkpoint_track_chunks() {
        let mut cp = TransferCheckpoint::new(Uuid::new_v4(), 10);
        assert_eq!(cp.remaining(), 10);
        assert!(!cp.is_complete(0));

        cp.mark_complete(0);
        cp.mark_complete(5);
        cp.mark_complete(9);

        assert!(cp.is_complete(0));
        assert!(cp.is_complete(5));
        assert!(!cp.is_complete(3));
        assert_eq!(cp.remaining(), 7);
    }

    #[test]
    fn checkpoint_missing_chunks() {
        let mut cp = TransferCheckpoint::new(Uuid::new_v4(), 5);
        cp.mark_complete(0);
        cp.mark_complete(2);
        cp.mark_complete(4);
        assert_eq!(cp.missing_chunks(), vec![1, 3]);
    }

    #[test]
    fn checkpoint_serde_roundtrip() {
        let mut cp = TransferCheckpoint::new(Uuid::new_v4(), 100);
        cp.mark_complete(42);
        cp.mark_complete(99);

        let json = serde_json::to_string(&cp).ok().unwrap();
        let back: TransferCheckpoint = serde_json::from_str(&json).ok().unwrap();
        assert!(back.is_complete(42));
        assert!(back.is_complete(99));
        assert!(!back.is_complete(50));
    }
}
