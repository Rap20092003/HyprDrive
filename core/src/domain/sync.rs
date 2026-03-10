//! Sync primitives — Vector Clocks and sync operations.
//!
//! These types enable leaderless, conflict-free replication
//! between HyprDrive devices without a central server.

use crate::domain::id::DeviceId;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use ulid::Ulid;

/// A ULID-stamped operation record for the sync log.
///
/// ULIDs are lexicographically ordered AND temporally ordered,
/// so sorting by ID ≈ sorting by time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncOperation {
    /// Unique lexicographically-sortable ID.
    pub id: Ulid,
    /// Which device generated this operation.
    pub device_id: DeviceId,
    /// Type of operation (e.g., "file.move", "tag.add").
    pub operation: String,
    /// Serialized operation payload.
    pub data: String,
    /// Vector clock at the time of this operation.
    pub clock: VectorClock,
}

/// A vector clock for causal ordering of distributed events.
///
/// Each device maintains a counter. Merge takes the max of each entry.
/// Concurrent events have incomparable clocks (neither before nor after).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorClock {
    entries: BTreeMap<DeviceId, u64>,
}

impl VectorClock {
    /// Create an empty vector clock.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Increment the counter for a device (typically "this" device).
    pub fn increment(&mut self, device_id: DeviceId) {
        let counter = self.entries.entry(device_id).or_insert(0);
        *counter += 1;
    }

    /// Get the counter for a specific device.
    pub fn get(&self, device_id: &DeviceId) -> u64 {
        self.entries.get(device_id).copied().unwrap_or(0)
    }

    /// Merge another clock into this one (take max of each entry).
    pub fn merge(&mut self, other: &VectorClock) {
        for (device, &count) in &other.entries {
            let entry = self.entries.entry(*device).or_insert(0);
            if count > *entry {
                *entry = count;
            }
        }
    }

    /// Number of devices tracked.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no devices are tracked.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Determine causal ordering between two clocks.
    ///
    /// Returns:
    /// - `Some(Less)` if self happened-before other
    /// - `Some(Greater)` if self happened-after other
    /// - `Some(Equal)` if identical
    /// - `None` if concurrent (neither before nor after)
    pub fn partial_order(&self, other: &VectorClock) -> Option<Ordering> {
        let all_keys: std::collections::BTreeSet<&DeviceId> = self
            .entries
            .keys()
            .chain(other.entries.keys())
            .collect();

        let mut has_less = false;
        let mut has_greater = false;

        for key in all_keys {
            let a = self.get(key);
            let b = other.get(key);
            match a.cmp(&b) {
                Ordering::Less => has_less = true,
                Ordering::Greater => has_greater = true,
                Ordering::Equal => {}
            }
            if has_less && has_greater {
                return None; // concurrent
            }
        }

        match (has_less, has_greater) {
            (false, false) => Some(Ordering::Equal),
            (true, false) => Some(Ordering::Less),
            (false, true) => Some(Ordering::Greater),
            (true, true) => None, // should be caught above
        }
    }
}

impl Default for VectorClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ulid_lexicographic_is_temporal() {
        let id1 = Ulid::new();
        // Tiny sleep to ensure different timestamps
        std::thread::sleep(std::time::Duration::from_millis(2));
        let id2 = Ulid::new();
        assert!(id1 < id2, "ULID should be lexicographically temporal");
    }

    #[test]
    fn vector_clock_increment() {
        let device = DeviceId::new();
        let mut clock = VectorClock::new();
        assert_eq!(clock.get(&device), 0);

        clock.increment(device);
        assert_eq!(clock.get(&device), 1);

        clock.increment(device);
        assert_eq!(clock.get(&device), 2);
    }

    #[test]
    fn vector_clock_merge_takes_max() {
        let d1 = DeviceId::new();
        let d2 = DeviceId::new();

        let mut clock_a = VectorClock::new();
        clock_a.increment(d1);
        clock_a.increment(d1);
        clock_a.increment(d2);

        let mut clock_b = VectorClock::new();
        clock_b.increment(d1);
        clock_b.increment(d2);
        clock_b.increment(d2);
        clock_b.increment(d2);

        clock_a.merge(&clock_b);
        assert_eq!(clock_a.get(&d1), 2); // max(2, 1) = 2
        assert_eq!(clock_a.get(&d2), 3); // max(1, 3) = 3
    }

    #[test]
    fn vector_clock_concurrent_detection() {
        let d1 = DeviceId::new();
        let d2 = DeviceId::new();

        let mut clock_a = VectorClock::new();
        clock_a.increment(d1); // A: {d1: 1}

        let mut clock_b = VectorClock::new();
        clock_b.increment(d2); // B: {d2: 1}

        // Neither is before the other → concurrent
        assert_eq!(clock_a.partial_order(&clock_b), None);
    }

    #[test]
    fn vector_clock_happens_before() {
        let d1 = DeviceId::new();

        let mut clock_a = VectorClock::new();
        clock_a.increment(d1); // {d1: 1}

        let mut clock_b = clock_a.clone();
        clock_b.increment(d1); // {d1: 2}

        assert_eq!(clock_a.partial_order(&clock_b), Some(Ordering::Less));
        assert_eq!(clock_b.partial_order(&clock_a), Some(Ordering::Greater));
    }

    #[test]
    fn vector_clock_equal() {
        let d1 = DeviceId::new();
        let mut clock_a = VectorClock::new();
        clock_a.increment(d1);

        let clock_b = clock_a.clone();
        assert_eq!(clock_a.partial_order(&clock_b), Some(Ordering::Equal));
    }

    #[test]
    fn sync_operation_serde_roundtrip() {
        let op = SyncOperation {
            id: Ulid::new(),
            device_id: DeviceId::new(),
            operation: "file.move".into(),
            data: r#"{"from": "/a", "to": "/b"}"#.into(),
            clock: VectorClock::new(),
        };
        let json = serde_json::to_string(&op).ok().unwrap();
        let back: SyncOperation = serde_json::from_str(&json).ok().unwrap();
        assert_eq!(op.operation, back.operation);
    }
}
