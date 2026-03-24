//! Real-time USN journal listener for continuous filesystem monitoring.
//!
//! Spawns a background thread per monitored NTFS volume that polls the USN
//! journal at a configurable interval (default 100ms). Change events are sent
//! via a `tokio::sync::mpsc` channel to async consumers.
//!
//! Cursor persistence is handled via a [`CursorStore`] trait so that the
//! listener does not depend on a specific storage backend.

use crate::error::FsIndexerResult;
use crate::types::{CursorStore, FsChange, UsnCursor};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Configuration for the USN journal listener.
#[derive(Debug, Clone)]
pub struct ListenerConfig {
    /// How often to poll the USN journal (default: 100ms).
    pub poll_interval: Duration,
    /// Capacity of the mpsc channel (default: 10,000).
    pub channel_capacity: usize,
    /// Volumes to monitor (e.g. `["C:\\", "D:\\"]`).
    pub volumes: Vec<PathBuf>,
}

impl Default for ListenerConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(100),
            channel_capacity: 10_000,
            volumes: Vec::new(),
        }
    }
}

impl ListenerConfig {
    /// Set the polling interval.
    #[must_use]
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Set the channel capacity.
    #[must_use]
    pub fn with_capacity(mut self, capacity: usize) -> Self {
        self.channel_capacity = capacity;
        self
    }

    /// Add a volume to monitor.
    #[must_use]
    pub fn add_volume(mut self, volume: PathBuf) -> Self {
        self.volumes.push(volume);
        self
    }
}

/// Real-time USN journal listener.
///
/// Monitors NTFS volumes for filesystem changes by continuously polling the
/// USN journal. Events are sent via an `mpsc` channel. Cursor state is
/// persisted via a [`CursorStore`] for crash recovery.
pub struct UsnListener {
    config: ListenerConfig,
    tx: mpsc::Sender<FsChange>,
    cancel: CancellationToken,
}

impl UsnListener {
    /// Create a new listener with the given configuration.
    ///
    /// Returns the listener and a receiver for filesystem change events.
    pub fn new(config: ListenerConfig) -> (Self, mpsc::Receiver<FsChange>) {
        let (tx, rx) = mpsc::channel(config.channel_capacity);
        let listener = Self {
            config,
            tx,
            cancel: CancellationToken::new(),
        };
        (listener, rx)
    }

    /// Start monitoring all configured volumes.
    ///
    /// Spawns one background thread per volume. Each thread polls the USN
    /// journal at `config.poll_interval` and sends events via the channel.
    /// Cursor state is persisted via `store` after each batch.
    ///
    /// Returns a `JoinHandle` per volume for the caller to await or drop.
    #[tracing::instrument(skip(self, store), fields(volumes = ?self.config.volumes))]
    pub fn start<S: CursorStore>(&self, store: Arc<S>) -> FsIndexerResult<Vec<JoinHandle<()>>> {
        let mut handles = Vec::with_capacity(self.config.volumes.len());

        for volume in &self.config.volumes {
            let vol = volume.clone();
            let tx = self.tx.clone();
            let cancel = self.cancel.clone();
            let interval = self.config.poll_interval;
            let cursor_store = Arc::clone(&store);

            let handle = tokio::task::spawn_blocking(move || {
                poll_loop(vol, tx, cancel, cursor_store, interval);
            });

            handles.push(handle);
        }

        tracing::info!(volume_count = handles.len(), "USN listener started");

        Ok(handles)
    }

    /// Signal all background threads to stop.
    pub fn shutdown(&self) {
        tracing::info!("USN listener shutdown requested");
        self.cancel.cancel();
    }

    /// Check if shutdown has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

/// Save a `UsnCursor` to the store as JSON.
fn save_cursor<S: CursorStore>(
    store: &S,
    vkey: &str,
    cursor: &UsnCursor,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let json = serde_json::to_string(cursor)?;
    store.save(vkey, &json)
}

/// Load a `UsnCursor` from the store (deserializing from JSON).
fn load_cursor<S: CursorStore>(
    store: &S,
    vkey: &str,
) -> Result<Option<UsnCursor>, Box<dyn std::error::Error + Send + Sync>> {
    match store.load(vkey)? {
        Some(json) => Ok(Some(serde_json::from_str(&json)?)),
        None => Ok(None),
    }
}

/// Extract drive letter from a volume path for use as cursor store key.
fn volume_key(volume: &std::path::Path) -> String {
    let s = volume.to_string_lossy();
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        String::from(bytes[0] as char)
    } else {
        s.to_string()
    }
}

/// Main poll loop for a single volume. Runs in a blocking thread.
fn poll_loop<S: CursorStore>(
    volume: PathBuf,
    tx: mpsc::Sender<FsChange>,
    cancel: CancellationToken,
    store: Arc<S>,
    interval: Duration,
) {
    let vkey = volume_key(&volume);
    let _span = tracing::info_span!("usn_listener", volume = %vkey).entered();

    // Load cursor from store, or read a fresh one
    let mut cursor = match load_cursor(store.as_ref(), &vkey) {
        Ok(Some(c)) => {
            tracing::info!(
                journal_id = c.journal_id,
                next_usn = c.next_usn,
                "Resuming from persisted cursor"
            );
            c
        }
        Ok(None) => {
            tracing::info!("No persisted cursor, reading fresh cursor");
            match super::usn::read_cursor(&volume) {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to read initial cursor, listener exiting");
                    return;
                }
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load cursor from store, reading fresh");
            match super::usn::read_cursor(&volume) {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to read initial cursor, listener exiting");
                    return;
                }
            }
        }
    };

    loop {
        // Check for shutdown
        if cancel.is_cancelled() {
            tracing::info!("Shutdown signal received, persisting final cursor");
            if let Err(e) = save_cursor(store.as_ref(), &vkey, &cursor) {
                tracing::error!(error = %e, "Failed to persist final cursor");
            }
            return;
        }

        // Poll for changes
        match super::usn::poll_changes(&volume, &cursor) {
            Ok((changes, new_cursor)) => {
                // Detect journal_id mismatch (journal was recreated)
                if new_cursor.journal_id != cursor.journal_id && cursor.journal_id != 0 {
                    tracing::warn!(
                        old_journal_id = cursor.journal_id,
                        new_journal_id = new_cursor.journal_id,
                        "Journal ID changed, full rescan needed"
                    );
                    let rescan = FsChange::FullRescanNeeded {
                        volume: volume.clone(),
                        reason: format!(
                            "USN journal ID changed from {} to {}",
                            cursor.journal_id, new_cursor.journal_id
                        ),
                    };
                    if tx.blocking_send(rescan).is_err() {
                        tracing::warn!("Channel closed, listener exiting");
                        return;
                    }
                }

                // Send changes
                for change in &changes {
                    if tx.blocking_send(change.clone()).is_err() {
                        tracing::warn!("Channel closed, listener exiting");
                        return;
                    }
                }

                if !changes.is_empty() {
                    tracing::debug!(count = changes.len(), "Sent change events");
                }

                cursor = new_cursor;

                // Persist cursor after each batch
                if let Err(e) = save_cursor(store.as_ref(), &vkey, &cursor) {
                    tracing::warn!(error = %e, "Failed to persist cursor (will retry)");
                }
            }
            Err(e) => {
                // IO error — might be temporary (dismounted volume, etc.)
                tracing::warn!(error = %e, "Poll failed, will retry after interval");

                // Check if this is a journal-wrapped scenario
                let err_msg = e.to_string();
                if err_msg.contains("journal") || err_msg.contains("invalid") {
                    let rescan = FsChange::FullRescanNeeded {
                        volume: volume.clone(),
                        reason: format!("USN poll error: {e}"),
                    };
                    if tx.blocking_send(rescan).is_err() {
                        tracing::warn!("Channel closed, listener exiting");
                        return;
                    }

                    // Try to read a fresh cursor
                    match super::usn::read_cursor(&volume) {
                        Ok(fresh) => {
                            tracing::info!("Obtained fresh cursor after error");
                            cursor = fresh;
                        }
                        Err(e2) => {
                            tracing::error!(error = %e2, "Failed to read fresh cursor");
                        }
                    }
                }
            }
        }

        // Sleep for the configured interval
        std::thread::sleep(interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listener_config_defaults() {
        let config = ListenerConfig::default();
        assert_eq!(config.poll_interval, Duration::from_millis(100));
        assert_eq!(config.channel_capacity, 10_000);
        assert!(config.volumes.is_empty());
    }

    #[test]
    fn listener_config_builder() {
        let config = ListenerConfig::default()
            .with_poll_interval(Duration::from_millis(50))
            .with_capacity(5_000)
            .add_volume(PathBuf::from("C:\\"))
            .add_volume(PathBuf::from("D:\\"));

        assert_eq!(config.poll_interval, Duration::from_millis(50));
        assert_eq!(config.channel_capacity, 5_000);
        assert_eq!(config.volumes.len(), 2);
    }

    #[test]
    fn listener_new_creates_valid_pair() {
        let config = ListenerConfig::default().add_volume(PathBuf::from("C:\\"));
        let (listener, _rx) = UsnListener::new(config);
        assert!(!listener.is_cancelled());
        assert_eq!(listener.config.volumes.len(), 1);
    }

    #[test]
    fn listener_shutdown_sets_cancelled() {
        let config = ListenerConfig::default();
        let (listener, _rx) = UsnListener::new(config);
        assert!(!listener.is_cancelled());
        listener.shutdown();
        assert!(listener.is_cancelled());
    }

    #[test]
    fn volume_key_extraction() {
        assert_eq!(volume_key(std::path::Path::new("C:\\")), "C");
        assert_eq!(volume_key(std::path::Path::new("D:\\")), "D");
        assert_eq!(volume_key(std::path::Path::new("/mnt/data")), "/mnt/data");
    }

    #[test]
    fn listener_multi_volume_config() {
        let config = ListenerConfig::default()
            .add_volume(PathBuf::from("C:\\"))
            .add_volume(PathBuf::from("D:\\"))
            .add_volume(PathBuf::from("E:\\"));

        let (listener, _rx) = UsnListener::new(config);
        assert_eq!(listener.config.volumes.len(), 3);
    }

    #[test]
    fn no_cursor_store_works() {
        use crate::types::NoCursorStore;
        let store = NoCursorStore;
        let cursor = UsnCursor {
            journal_id: 42,
            next_usn: 100,
        };
        save_cursor(&store, "C", &cursor).unwrap();
        assert_eq!(load_cursor(&store, "C").unwrap(), None);
    }

    /// In-memory cursor store for testing. Stores cursors as JSON strings in a HashMap.
    struct InMemoryCursorStore {
        data: std::sync::Mutex<std::collections::HashMap<String, String>>,
    }

    impl InMemoryCursorStore {
        fn new() -> Self {
            Self {
                data: std::sync::Mutex::new(std::collections::HashMap::new()),
            }
        }
    }

    impl CursorStore for InMemoryCursorStore {
        fn save(
            &self,
            volume_key: &str,
            cursor_json: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.data
                .lock()
                .unwrap()
                .insert(volume_key.to_string(), cursor_json.to_string());
            Ok(())
        }

        fn load(
            &self,
            volume_key: &str,
        ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(self.data.lock().unwrap().get(volume_key).cloned())
        }
    }

    #[test]
    fn in_memory_cursor_store_roundtrip() {
        let store = InMemoryCursorStore::new();
        let cursor = UsnCursor {
            journal_id: 42,
            next_usn: 100,
        };
        save_cursor(&store, "C", &cursor).unwrap();
        let loaded = load_cursor(&store, "C").unwrap();
        assert_eq!(loaded, Some(cursor));
    }

    #[test]
    fn in_memory_cursor_store_miss() {
        let store = InMemoryCursorStore::new();
        assert_eq!(load_cursor(&store, "Z").unwrap(), None);
    }

    #[test]
    fn in_memory_cursor_store_overwrite() {
        let store = InMemoryCursorStore::new();
        save_cursor(
            &store,
            "C",
            &UsnCursor {
                journal_id: 1,
                next_usn: 10,
            },
        )
        .unwrap();
        save_cursor(
            &store,
            "C",
            &UsnCursor {
                journal_id: 1,
                next_usn: 50,
            },
        )
        .unwrap();
        let loaded = load_cursor(&store, "C").unwrap().unwrap();
        assert_eq!(loaded.next_usn, 50);
    }

    /// Integration test: requires admin privileges.
    /// Run: `cargo test -p hyprdrive-fs-indexer -- --ignored listener_start_and_shutdown`
    #[tokio::test]
    #[ignore]
    async fn listener_start_and_shutdown() {
        let store = Arc::new(crate::NoCursorStore);

        let config = ListenerConfig::default()
            .with_poll_interval(Duration::from_millis(50))
            .add_volume(PathBuf::from("C:\\"));

        let (listener, mut rx) = UsnListener::new(config);
        let handles = listener.start(store).unwrap();

        // Create a temp file to trigger a change
        let test_file = std::env::temp_dir().join("hyprdrive_listener_test.tmp");
        tokio::fs::write(&test_file, b"listener test")
            .await
            .unwrap();

        // Wait for an event (with timeout)
        let result = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await;

        // Cleanup
        let _ = tokio::fs::remove_file(&test_file).await;
        listener.shutdown();

        for handle in handles {
            let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
        }

        // We should have received at least one event
        assert!(
            result.is_ok(),
            "Expected to receive a change event within 2s"
        );
    }
}
