//! Watcher loop — debounce, coalesce, and dispatch FsChange events.
//!
//! Raw USN events are noisy (a single save can produce multiple Modified events).
//! This module collects events over a debounce window, deduplicates them via
//! `coalesce_changes`, then dispatches the batch to the ChangeProcessor.

use hyprdrive_fs_indexer::{FsChange, VolumedChange};
use hyprdrive_object_pipeline::ChangeProcessor;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;

/// Default debounce window.
const DEFAULT_DEBOUNCE_MS: u64 = 300;
/// Default max batch size before forcing a dispatch.
const DEFAULT_MAX_BATCH: usize = 5000;

/// Event loop that collects, coalesces, and dispatches FsChange batches.
pub struct WatcherLoop {
    rx: mpsc::Receiver<VolumedChange>,
    processors: HashMap<String, Arc<ChangeProcessor>>,
    debounce: Duration,
    max_batch: usize,
    rescan_tx: mpsc::Sender<PathBuf>,
}

impl WatcherLoop {
    /// Create a watcher with a single processor (backward compat).
    #[allow(dead_code)]
    pub fn new(
        rx: mpsc::Receiver<VolumedChange>,
        processor: Arc<ChangeProcessor>,
        volume_id: String,
        rescan_tx: mpsc::Sender<PathBuf>,
    ) -> Self {
        let mut processors = HashMap::new();
        processors.insert(volume_id, processor);
        Self {
            rx,
            processors,
            debounce: Duration::from_millis(DEFAULT_DEBOUNCE_MS),
            max_batch: DEFAULT_MAX_BATCH,
            rescan_tx,
        }
    }

    /// Create a watcher with multiple processors (one per volume).
    pub fn new_multi(
        rx: mpsc::Receiver<VolumedChange>,
        processors: HashMap<String, Arc<ChangeProcessor>>,
        rescan_tx: mpsc::Sender<PathBuf>,
    ) -> Self {
        Self {
            rx,
            processors,
            debounce: Duration::from_millis(DEFAULT_DEBOUNCE_MS),
            max_batch: DEFAULT_MAX_BATCH,
            rescan_tx,
        }
    }

    /// Main loop: collect → partition by volume → coalesce → dispatch → repeat.
    pub async fn run(&mut self) {
        let mut batch: Vec<VolumedChange> = Vec::new();

        loop {
            // Wait for first event or channel close.
            let first = match self.rx.recv().await {
                Some(ev) => ev,
                None => break, // channel closed = shutdown
            };
            batch.push(first);

            // Drain more events for debounce window, up to max_batch.
            let deadline = Instant::now() + self.debounce;
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() || batch.len() >= self.max_batch {
                    break;
                }
                match tokio::time::timeout(remaining, self.rx.recv()).await {
                    Ok(Some(ev)) => batch.push(ev),
                    Ok(None) => return, // channel closed
                    Err(_) => break,    // timeout — debounce window expired
                }
            }

            // Partition by volume_id.
            let mut by_volume: HashMap<String, Vec<FsChange>> = HashMap::new();
            for vc in std::mem::take(&mut batch) {
                by_volume.entry(vc.volume_id).or_default().push(vc.change);
            }

            // Coalesce and dispatch per volume.
            for (vol_id, changes) in by_volume {
                let coalesced = coalesce_changes(changes);
                if coalesced.is_empty() {
                    continue;
                }

                let processor = match self.processors.get(&vol_id) {
                    Some(p) => Arc::clone(p),
                    None => {
                        tracing::warn!(
                            volume = %vol_id,
                            dropped = coalesced.len(),
                            "no processor for volume — dropping events"
                        );
                        continue;
                    }
                };

                match processor.process_changes(coalesced).await {
                    Ok(stats) => {
                        tracing::info!(
                            volume = %vol_id,
                            created = stats.created,
                            deleted = stats.deleted,
                            moved = stats.moved,
                            modified = stats.modified,
                            errors = stats.errors,
                            "watcher batch processed"
                        );
                        if stats.rescan_needed {
                            // Signal daemon to re-scan — best effort.
                            let _ = self.rescan_tx.try_send(PathBuf::new());
                        }
                    }
                    Err(e) => {
                        tracing::error!(volume = %vol_id, error = %e, "watcher batch failed");
                    }
                }
            }
        }

        tracing::info!("watcher loop exiting (channel closed)");
    }
}

/// Extract the fid from an FsChange event, if applicable.
fn change_fid(change: &FsChange) -> Option<u64> {
    match change {
        FsChange::Created(entry) => Some(entry.fid),
        FsChange::Deleted { fid, .. } => Some(*fid),
        FsChange::Moved { fid, .. } => Some(*fid),
        FsChange::Modified { fid, .. } => Some(*fid),
        FsChange::FullRescanNeeded { .. } => None,
    }
}

/// Deduplicate a batch of FsChange events using sequential state reduction.
///
/// Rules:
/// - Multiple Modified for same fid → keep last
/// - Created + Deleted for same fid → cancel both
/// - Deleted then Created for same fid → treat as Modified
/// - Moved events are always preserved (never collapsed with Modified)
/// - FullRescanNeeded always passes through
pub fn coalesce_changes(changes: Vec<FsChange>) -> Vec<FsChange> {
    // Track per-fid state by folding events left-to-right.
    // This correctly handles arbitrary multi-event sequences (e.g., Created→Deleted→Created).
    let mut fid_state: HashMap<u64, CoalescedState> = HashMap::new();
    let mut result: Vec<FsChange> = Vec::new();

    for change in changes {
        match change_fid(&change) {
            Some(fid) => {
                let state = fid_state.entry(fid).or_insert(CoalescedState::Empty);
                let prev = std::mem::replace(state, CoalescedState::Empty);
                *state = prev.fold(change);
            }
            None => result.push(change), // FullRescanNeeded
        }
    }

    for (_fid, state) in fid_state {
        match state {
            CoalescedState::Empty | CoalescedState::Cancelled => {}
            CoalescedState::Single(ev) => result.push(ev),
            CoalescedState::MovedThenModified { moved, modified } => {
                result.push(moved);
                result.push(modified);
            }
        }
    }

    result
}

/// Intermediate state for a single fid during coalescing.
enum CoalescedState {
    /// No events yet.
    Empty,
    /// Created+Deleted cancelled each other out.
    Cancelled,
    /// A single surviving event (latest wins for same-type events).
    Single(FsChange),
    /// A Moved followed by a Modified — both must be emitted.
    MovedThenModified { moved: FsChange, modified: FsChange },
}

impl CoalescedState {
    /// Fold a new event into the current state.
    fn fold(self, event: FsChange) -> Self {
        match (self, &event) {
            // ── From Empty ──
            (CoalescedState::Empty, _) => CoalescedState::Single(event),

            // ── Created then Deleted → cancel ──
            (CoalescedState::Single(FsChange::Created(_)), FsChange::Deleted { .. }) => {
                CoalescedState::Cancelled
            }

            // ── Deleted then Created → treat as Modified ──
            (CoalescedState::Single(FsChange::Deleted { .. }), FsChange::Created(entry)) => {
                CoalescedState::Single(FsChange::Modified {
                    fid: entry.fid,
                    new_size: entry.size,
                })
            }

            // ── Cancelled then Created → revive as Created ──
            (CoalescedState::Cancelled, FsChange::Created(_)) => CoalescedState::Single(event),

            // ── Cancelled then anything else → just that event ──
            (CoalescedState::Cancelled, _) => CoalescedState::Single(event),

            // ── Created then Modified → keep Created (it has full data; file is new) ──
            (CoalescedState::Single(created @ FsChange::Created(_)), FsChange::Modified { .. }) => {
                CoalescedState::Single(created)
            }

            // ── Moved then Modified → preserve both ──
            (CoalescedState::Single(moved @ FsChange::Moved { .. }), FsChange::Modified { .. }) => {
                CoalescedState::MovedThenModified {
                    moved,
                    modified: event,
                }
            }

            // ── MovedThenModified + newer Modified → update the Modified ──
            (CoalescedState::MovedThenModified { moved, .. }, FsChange::Modified { .. }) => {
                CoalescedState::MovedThenModified {
                    moved,
                    modified: event,
                }
            }

            // ── MovedThenModified + Deleted → cancel everything ──
            (CoalescedState::MovedThenModified { .. }, FsChange::Deleted { .. }) => {
                CoalescedState::Cancelled
            }

            // ── Same-type or other combinations → last event wins ──
            (_, _) => CoalescedState::Single(event),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use chrono::Utc;
    use hyprdrive_fs_indexer::IndexEntry;
    use std::ffi::OsString;

    fn make_entry(fid: u64) -> IndexEntry {
        IndexEntry {
            fid,
            parent_fid: 0,
            name: OsString::from("test.txt"),
            name_lossy: "test.txt".to_string(),
            full_path: PathBuf::from("/test.txt"),
            size: 100,
            allocated_size: 4096,
            is_dir: false,
            modified_at: Utc::now(),
            attributes: 0,
        }
    }

    #[test]
    fn test_coalesce_empty() {
        let result = coalesce_changes(vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_coalesce_dedup_modified() {
        let changes = vec![
            FsChange::Modified {
                fid: 42,
                new_size: 100,
            },
            FsChange::Modified {
                fid: 42,
                new_size: 200,
            },
        ];
        let result = coalesce_changes(changes);
        assert_eq!(result.len(), 1);
        match &result[0] {
            FsChange::Modified { fid, new_size } => {
                assert_eq!(*fid, 42);
                assert_eq!(*new_size, 200);
            }
            _ => panic!("expected Modified"),
        }
    }

    #[test]
    fn test_coalesce_create_delete_cancel() {
        let changes = vec![
            FsChange::Created(make_entry(42)),
            FsChange::Deleted {
                fid: 42,
                path: None,
            },
        ];
        let result = coalesce_changes(changes);
        assert!(result.is_empty(), "Created+Deleted same fid should cancel");
    }

    #[test]
    fn test_coalesce_delete_create_becomes_modified() {
        let changes = vec![
            FsChange::Deleted {
                fid: 42,
                path: None,
            },
            FsChange::Created(make_entry(42)),
        ];
        let result = coalesce_changes(changes);
        assert_eq!(result.len(), 1);
        assert!(
            matches!(&result[0], FsChange::Modified { fid: 42, .. }),
            "Delete+Create should become Modified"
        );
    }

    #[test]
    fn test_coalesce_preserves_different_fids() {
        let changes = vec![
            FsChange::Modified {
                fid: 1,
                new_size: 100,
            },
            FsChange::Modified {
                fid: 2,
                new_size: 200,
            },
        ];
        let result = coalesce_changes(changes);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_coalesce_moved_then_modified_preserves_both() {
        let changes = vec![
            FsChange::Moved {
                fid: 42,
                new_parent_fid: 10,
                new_name: "renamed.txt".into(),
            },
            FsChange::Modified {
                fid: 42,
                new_size: 200,
            },
        ];
        let result = coalesce_changes(changes);
        assert_eq!(result.len(), 2, "Moved+Modified should produce two events");
        assert!(
            result.iter().any(|e| matches!(e, FsChange::Moved { .. })),
            "should contain Moved"
        );
        assert!(
            result
                .iter()
                .any(|e| matches!(e, FsChange::Modified { .. })),
            "should contain Modified"
        );
    }

    #[test]
    fn test_coalesce_create_delete_create_revives() {
        // Created→Deleted→Created for same fid: first pair cancels, second Created survives.
        let changes = vec![
            FsChange::Created(make_entry(42)),
            FsChange::Deleted {
                fid: 42,
                path: None,
            },
            FsChange::Created(make_entry(42)),
        ];
        let result = coalesce_changes(changes);
        assert_eq!(result.len(), 1);
        assert!(
            matches!(&result[0], FsChange::Created(_)),
            "should revive as Created after cancel+re-create"
        );
    }

    #[test]
    fn test_coalesce_created_then_modified_keeps_created() {
        // inotify fires both CREATE and MODIFY for a new file write.
        // The coalescer should keep Created (it has full data).
        let changes = vec![
            FsChange::Created(make_entry(42)),
            FsChange::Modified {
                fid: 42,
                new_size: 200,
            },
        ];
        let result = coalesce_changes(changes);
        assert_eq!(result.len(), 1);
        assert!(
            matches!(&result[0], FsChange::Created(_)),
            "Created+Modified should keep Created"
        );
    }

    #[test]
    fn test_coalesce_preserves_rescan() {
        let changes = vec![
            FsChange::Modified {
                fid: 1,
                new_size: 100,
            },
            FsChange::FullRescanNeeded {
                volume: PathBuf::from("C:\\"),
                reason: "journal wrapped".to_string(),
            },
        ];
        let result = coalesce_changes(changes);
        assert_eq!(result.len(), 2);
        assert!(result
            .iter()
            .any(|e| matches!(e, FsChange::FullRescanNeeded { .. })));
    }
}
