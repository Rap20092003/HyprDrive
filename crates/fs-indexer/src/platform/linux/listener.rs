//! Real-time filesystem monitoring via inotify.
//!
//! Spawns a background task that watches a directory tree using `inotify`
//! and sends [`FsChange`] events via a `tokio::sync::mpsc` channel.
//!
//! Architecture mirrors the Windows [`UsnListener`]:
//! - Recursive watch setup on init
//! - Background event loop reading inotify events
//! - Move cookie pairing for `IN_MOVED_FROM`/`IN_MOVED_TO`
//! - Graceful shutdown via [`CancellationToken`]
//!
//! Future: fanotify upgrade behind `fanotify` feature flag when
//! `CAP_SYS_ADMIN` is available.

use crate::error::{FsIndexerError, FsIndexerResult};
use crate::types::{FsChange, IndexEntry};
use chrono::Utc;
use inotify::{EventMask, Inotify, WatchDescriptor, WatchMask};
use std::collections::HashMap;
use std::ffi::OsString;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::walk;

/// Configuration for the Linux filesystem listener.
#[derive(Debug, Clone)]
pub struct LinuxListenerConfig {
    /// Root directory to watch.
    pub root: PathBuf,
    /// Capacity of the event channel (default: 10,000).
    pub channel_capacity: usize,
    /// Whether to recursively watch subdirectories (default: true).
    pub recursive: bool,
}

impl Default for LinuxListenerConfig {
    fn default() -> Self {
        Self {
            root: PathBuf::from("/"),
            channel_capacity: 10_000,
            recursive: true,
        }
    }
}

/// Real-time Linux filesystem change listener using inotify.
///
/// Monitors a directory tree for creates, deletes, modifications, and moves.
/// Events are sent via an mpsc channel as [`FsChange`] values.
pub struct LinuxListener {
    config: LinuxListenerConfig,
    tx: mpsc::Sender<FsChange>,
    cancel: CancellationToken,
}

impl LinuxListener {
    /// Create a new listener, returning the listener and a receiver for events.
    pub fn new(config: LinuxListenerConfig) -> (Self, mpsc::Receiver<FsChange>) {
        let (tx, rx) = mpsc::channel(config.channel_capacity);
        let listener = Self {
            config,
            tx,
            cancel: CancellationToken::new(),
        };
        (listener, rx)
    }

    /// Start the listener, spawning a background task.
    ///
    /// Recursively adds inotify watches on all directories under the root,
    /// then enters an event loop that reads inotify events and sends
    /// [`FsChange`] values through the channel.
    pub fn start(&self) -> FsIndexerResult<JoinHandle<()>> {
        let (inotify, watch_map) = setup_watches(&self.config.root, self.config.recursive)?;
        let tx = self.tx.clone();
        let cancel = self.cancel.clone();
        let root = self.config.root.clone();
        let recursive = self.config.recursive;

        let handle = tokio::spawn(async move {
            event_loop(inotify, watch_map, tx, cancel, root, recursive).await;
        });

        Ok(handle)
    }

    /// Signal the listener to stop.
    ///
    /// The background task will exit on its next iteration.
    pub fn shutdown(&self) {
        self.cancel.cancel();
    }
}

/// Read the kernel's max inotify watches limit.
fn read_max_watches() -> Option<usize> {
    std::fs::read_to_string("/proc/sys/fs/inotify/max_user_watches")
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
}

/// Build the standard watch mask for all inotify watches.
fn watch_mask() -> WatchMask {
    WatchMask::CREATE
        | WatchMask::DELETE
        | WatchMask::MODIFY
        | WatchMask::MOVED_FROM
        | WatchMask::MOVED_TO
        | WatchMask::ATTRIB
        | WatchMask::DELETE_SELF
}

/// Collect directories to watch under `root`.
fn collect_watch_dirs(root: &Path, recursive: bool) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = vec![root.to_path_buf()];
    if recursive {
        for entry in jwalk::WalkDir::new(root)
            .skip_hidden(false)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if entry.file_type().is_dir() && entry.depth() > 0 {
                dirs.push(entry.path());
            }
        }
    }
    dirs
}

/// Add a single inotify watch, returning the watch descriptor or an error.
fn add_single_watch(
    inotify: &mut Inotify,
    dir: &Path,
    mask: WatchMask,
) -> Result<WatchDescriptor, std::io::Error> {
    let mut watches = inotify.watches();
    watches.add(dir, mask)
}

/// Set up recursive inotify watches on a directory tree.
///
/// Separated into small helper functions to work around a rustc ICE
/// in `mir_borrowck` (affects stable 1.94 and nightly 1.96).
fn setup_watches(
    root: &Path,
    recursive: bool,
) -> FsIndexerResult<(Inotify, HashMap<WatchDescriptor, PathBuf>)> {
    let mut inotify = Inotify::init().map_err(|e| FsIndexerError::FanotifyError { source: e })?;
    let mut watch_map = HashMap::new();
    let mask = watch_mask();

    let dirs_to_watch = collect_watch_dirs(root, recursive);

    for dir in &dirs_to_watch {
        match add_single_watch(&mut inotify, dir, mask) {
            Ok(wd) => {
                watch_map.insert(wd, dir.clone());
            }
            Err(e) => {
                if e.raw_os_error() == Some(nix::libc::ENOSPC) {
                    let max = read_max_watches().unwrap_or(0);
                    return Err(FsIndexerError::InotifyWatchLimit {
                        current: watch_map.len(),
                        max,
                    });
                }
                // First directory (root) is required — subsequent are best-effort
                if watch_map.is_empty() {
                    return Err(FsIndexerError::FanotifyError { source: e });
                }
                tracing::warn!(
                    path = %dir.display(),
                    error = %e,
                    "failed to add inotify watch, skipping"
                );
            }
        }
    }

    // Warn if approaching limit
    if let Some(max) = read_max_watches() {
        let usage_pct = (watch_map.len() * 100) / max.max(1);
        if usage_pct > 80 {
            tracing::warn!(
                watches = watch_map.len(),
                max = max,
                usage_pct = usage_pct,
                "approaching inotify watch limit"
            );
        }
    }

    tracing::info!(watches = watch_map.len(), "inotify watches established");
    Ok((inotify, watch_map))
}

/// Main event reading loop — runs in a background task.
async fn event_loop(
    mut inotify: Inotify,
    mut watch_map: HashMap<WatchDescriptor, PathBuf>,
    tx: mpsc::Sender<FsChange>,
    cancel: CancellationToken,
    root: PathBuf,
    recursive: bool,
) {
    let mut buffer = vec![0u8; 4096];
    // Buffer for pairing MOVED_FROM/MOVED_TO by cookie
    let mut move_buffer: HashMap<u32, (u64, PathBuf, OsString)> = HashMap::new();

    loop {
        if cancel.is_cancelled() {
            tracing::info!("listener shutdown requested");
            break;
        }

        // Read events (blocking in tokio::spawn context)
        let events = match inotify.read_events(&mut buffer) {
            Ok(events) => events,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No events ready — yield and retry
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                continue;
            }
            Err(e) => {
                tracing::error!(error = %e, "inotify read error");
                break;
            }
        };

        for event in events {
            // Handle overflow
            if event.mask.contains(EventMask::Q_OVERFLOW) {
                let change = FsChange::FullRescanNeeded {
                    volume: root.clone(),
                    reason: "inotify queue overflow".to_string(),
                };
                if tx.send(change).await.is_err() {
                    tracing::debug!("receiver dropped, stopping listener");
                    return;
                }
                continue;
            }

            // Look up the watched directory path
            let watched_dir = match watch_map.get(&event.wd) {
                Some(p) => p.clone(),
                None => continue,
            };

            let event_name = event.name.map(|n| n.to_os_string());
            let full_path = if let Some(ref name) = event_name {
                watched_dir.join(name)
            } else {
                watched_dir.clone()
            };

            // Handle DELETE_SELF / IGNORED — cleanup watch
            if event.mask.contains(EventMask::DELETE_SELF)
                || event.mask.contains(EventMask::IGNORED)
            {
                watch_map.remove(&event.wd);
                continue;
            }

            // CREATE
            if event.mask.contains(EventMask::CREATE) {
                if let Ok(meta) = std::fs::symlink_metadata(&full_path) {
                    let dev = meta.dev();
                    let ino = meta.ino();
                    let fid = walk::make_fid(dev, ino);
                    let is_dir = meta.is_dir();

                    // Add inotify watch for new directories
                    if is_dir && recursive {
                        if let Ok(wd) = inotify.watches().add(&full_path, watch_mask()) {
                            watch_map.insert(wd, full_path.clone());
                        }
                    }

                    let name = event_name.clone().unwrap_or_default();
                    let entry = IndexEntry {
                        fid,
                        parent_fid: 0, // Not tracked in listener
                        name: name.clone(),
                        name_lossy: name.to_string_lossy().to_string(),
                        full_path: full_path.clone(),
                        size: meta.len(),
                        allocated_size: meta.blocks() * 512,
                        is_dir,
                        modified_at: Utc::now(),
                        attributes: 0,
                    };
                    if tx.send(FsChange::Created(entry)).await.is_err() {
                        return;
                    }
                }
            }

            // DELETE
            if event.mask.contains(EventMask::DELETE) {
                // We can't stat a deleted file, so use a hash-based fid
                let fid = fid_from_path(&full_path);
                if tx.send(FsChange::Deleted { fid }).await.is_err() {
                    return;
                }
            }

            // MODIFY
            if event.mask.contains(EventMask::MODIFY) {
                if let Ok(meta) = std::fs::symlink_metadata(&full_path) {
                    let fid = walk::make_fid(meta.dev(), meta.ino());
                    if tx
                        .send(FsChange::Modified {
                            fid,
                            new_size: meta.len(),
                        })
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }

            // MOVED_FROM — buffer for pairing with MOVED_TO
            if event.mask.contains(EventMask::MOVED_FROM) {
                let fid = fid_from_path(&full_path);
                let name = event_name.clone().unwrap_or_default();
                move_buffer.insert(event.cookie, (fid, full_path.clone(), name));
            }

            // MOVED_TO — pair with buffered MOVED_FROM
            if event.mask.contains(EventMask::MOVED_TO) {
                if let Some((_old_fid, _old_path, _old_name)) = move_buffer.remove(&event.cookie) {
                    // Paired move
                    if let Ok(meta) = std::fs::symlink_metadata(&full_path) {
                        let fid = walk::make_fid(meta.dev(), meta.ino());
                        let parent_meta = full_path
                            .parent()
                            .and_then(|p| std::fs::symlink_metadata(p).ok());
                        let new_parent_fid = parent_meta
                            .map(|m| walk::make_fid(m.dev(), m.ino()))
                            .unwrap_or(0);
                        let new_name = event_name.clone().unwrap_or_default();

                        if tx
                            .send(FsChange::Moved {
                                fid,
                                new_parent_fid,
                                new_name,
                            })
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                } else {
                    // Unpaired MOVED_TO — treat as Created
                    if let Ok(meta) = std::fs::symlink_metadata(&full_path) {
                        let fid = walk::make_fid(meta.dev(), meta.ino());
                        let name = event_name.clone().unwrap_or_default();
                        let entry = IndexEntry {
                            fid,
                            parent_fid: 0,
                            name: name.clone(),
                            name_lossy: name.to_string_lossy().to_string(),
                            full_path: full_path.clone(),
                            size: meta.len(),
                            allocated_size: meta.blocks() * 512,
                            is_dir: meta.is_dir(),
                            modified_at: Utc::now(),
                            attributes: 0,
                        };
                        if tx.send(FsChange::Created(entry)).await.is_err() {
                            return;
                        }
                    }
                }
            }
        }

        // Flush unpaired MOVED_FROM entries as deletes (simple approach)
        // In production, you'd use a timeout. For now, drain after each batch.
        for (_cookie, (fid, _path, _name)) in move_buffer.drain() {
            if tx.send(FsChange::Deleted { fid }).await.is_err() {
                return;
            }
        }
    }
}

/// Generate a fid from a path (for deleted files where we can't stat).
///
/// Uses a simple hash of the path bytes.
fn fid_from_path(path: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults() {
        let config = LinuxListenerConfig::default();
        assert_eq!(config.channel_capacity, 10_000);
        assert!(config.recursive);
    }

    #[test]
    fn listener_new_creates_pair() {
        let config = LinuxListenerConfig {
            root: PathBuf::from("/tmp"),
            channel_capacity: 100,
            recursive: false,
        };
        let (_listener, _rx) = LinuxListener::new(config);
        // If we get here without panic, it works
    }

    #[test]
    fn listener_shutdown_sets_cancelled() {
        let config = LinuxListenerConfig {
            root: PathBuf::from("/tmp"),
            ..LinuxListenerConfig::default()
        };
        let (listener, _rx) = LinuxListener::new(config);
        assert!(!listener.cancel.is_cancelled());
        listener.shutdown();
        assert!(listener.cancel.is_cancelled());
    }

    #[test]
    fn max_watches_parsing() {
        // This test is informational — it reads the actual system value
        if let Some(max) = read_max_watches() {
            assert!(max > 0, "max_user_watches should be positive");
        }
        // If /proc is not available (Windows), the function returns None
    }

    #[test]
    fn fid_from_path_deterministic() {
        let path = Path::new("/tmp/test.txt");
        let fid1 = fid_from_path(path);
        let fid2 = fid_from_path(path);
        assert_eq!(fid1, fid2, "same path should produce same fid");
    }

    #[test]
    fn fid_from_path_different() {
        let fid1 = fid_from_path(Path::new("/tmp/a.txt"));
        let fid2 = fid_from_path(Path::new("/tmp/b.txt"));
        assert_ne!(fid1, fid2, "different paths should produce different fids");
    }

    #[test]
    #[ignore] // Requires Linux + tokio runtime — run in WSL2
    fn listener_detects_create() {
        let rt = tokio::runtime::Runtime::new().expect("create runtime");
        rt.block_on(async {
            let dir = tempfile::TempDir::new().expect("create tempdir");
            let config = LinuxListenerConfig {
                root: dir.path().to_path_buf(),
                channel_capacity: 100,
                recursive: true,
            };
            let (listener, mut rx) = LinuxListener::new(config);
            let _handle = listener.start().expect("start listener");

            // Wait for watches to be established
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;

            // Create a file
            std::fs::write(dir.path().join("new_file.txt"), "hello").expect("write");

            // Wait for event
            let event = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await;
            assert!(event.is_ok(), "should receive event within timeout");
            if let Ok(Some(FsChange::Created(entry))) = event {
                assert_eq!(entry.name_lossy, "new_file.txt");
            }

            listener.shutdown();
        });
    }

    #[test]
    #[ignore] // Requires Linux + tokio runtime — run in WSL2
    fn listener_shutdown_clean() {
        let rt = tokio::runtime::Runtime::new().expect("create runtime");
        rt.block_on(async {
            let dir = tempfile::TempDir::new().expect("create tempdir");
            let config = LinuxListenerConfig {
                root: dir.path().to_path_buf(),
                channel_capacity: 100,
                recursive: false,
            };
            let (listener, _rx) = LinuxListener::new(config);
            let handle = listener.start().expect("start listener");

            listener.shutdown();

            let result = tokio::time::timeout(std::time::Duration::from_secs(5), handle).await;
            assert!(result.is_ok(), "listener should shut down within 5s");
        });
    }
}
