//! Live bubble-up delta propagation for directory sizes.
//!
//! When a file is created, deleted, or resized, the delta propagates to all
//! ancestor directories. This module computes the deltas; the caller applies
//! them to SQLite (dir_sizes) and redb (DIR_SIZE_CACHE).

use serde::{Deserialize, Serialize};

/// A delta to apply to one directory's size record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DirSizeDelta {
    pub location_id: String,
    pub file_count_delta: i64,
    pub bytes_delta: i64,
    pub allocated_delta: i64,
}

/// Compute deltas for all ancestors when a file changes.
///
/// `ancestor_ids` should be ordered child-to-root (immediate parent first).
/// Each ancestor gets the same delta — cumulative_allocated propagates all
/// the way up the tree.
pub fn compute_bubble_up(
    ancestor_ids: &[String],
    file_count_delta: i64,
    bytes_delta: i64,
    allocated_delta: i64,
) -> Vec<DirSizeDelta> {
    ancestor_ids
        .iter()
        .map(|id| DirSizeDelta {
            location_id: id.clone(),
            file_count_delta,
            bytes_delta,
            allocated_delta,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_created_propagates_to_all_ancestors() {
        let ancestors = vec!["dir_a".to_string(), "dir_b".to_string(), "root".to_string()];
        let deltas = compute_bubble_up(&ancestors, 1, 4096, 4096);
        assert_eq!(deltas.len(), 3);
        for d in &deltas {
            assert_eq!(d.file_count_delta, 1);
            assert_eq!(d.bytes_delta, 4096);
            assert_eq!(d.allocated_delta, 4096);
        }
        assert_eq!(deltas[0].location_id, "dir_a");
        assert_eq!(deltas[1].location_id, "dir_b");
        assert_eq!(deltas[2].location_id, "root");
    }

    #[test]
    fn file_deleted_propagates_negative_deltas() {
        let ancestors = vec!["parent".to_string()];
        let deltas = compute_bubble_up(&ancestors, -1, -8192, -8192);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].file_count_delta, -1);
        assert_eq!(deltas[0].bytes_delta, -8192);
    }

    #[test]
    fn empty_ancestors_returns_empty() {
        let deltas = compute_bubble_up(&[], 1, 100, 200);
        assert!(deltas.is_empty());
    }
}
