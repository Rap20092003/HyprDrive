//! HyprDrive Daemon — The System
//!
//! This is THE primary binary. All UIs are thin clients that connect here.
//! The daemon owns: database, indexing, sync, crypto, extensions, and HTTP API.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::dbg_macro,
    missing_docs
)]

#[cfg_attr(target_os = "macos", allow(dead_code))]
mod cursor_store;
#[cfg_attr(target_os = "macos", allow(dead_code))]
mod watcher;

use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::info;

/// Default scan root for Windows volumes.
#[cfg(target_os = "windows")]
const DEFAULT_SCAN_ROOT: &str = "C:\\";

/// Default scan root for Linux — user's home directory.
#[cfg(target_os = "linux")]
fn default_scan_root() -> std::path::PathBuf {
    dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/home"))
}

/// Derive a volume ID from a scan root path.
///
/// - Windows: "C" from "C:\\"
/// - Linux/macOS: last path component (e.g. "home" from "/home")
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn derive_volume_id(scan_root: &std::path::Path) -> String {
    let lossy = scan_root.to_string_lossy();
    // Windows: extract drive letter.
    if lossy.len() >= 2 && lossy.as_bytes()[1] == b':' {
        let ch = lossy.as_bytes()[0] as char;
        if ch.is_ascii_alphabetic() {
            return ch.to_ascii_uppercase().to_string();
        }
    }
    // Linux/macOS: use last path component or full path.
    scan_root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| {
            // Root "/" has no file_name — use "root".
            "root".to_string()
        })
}

/// Run a full scan and pipeline for a volume. Reusable for both initial startup and rescan.
#[cfg(any(target_os = "windows", target_os = "linux"))]
async fn run_full_scan(
    scan_root: &std::path::Path,
    pool: &sqlx::SqlitePool,
    cache: &Arc<redb::Database>,
) -> Result<hyprdrive_fs_indexer::ScanResult> {
    info!(root = %scan_root.display(), "starting volume scan...");
    let result = hyprdrive_fs_indexer::auto_scan(scan_root).context("volume scan failed")?;

    let volume_id = derive_volume_id(scan_root);
    info!(
        entries = result.entries.len(),
        has_usn_cursor = result.cursor.is_some(),
        has_linux_cursor = result.linux_cursor.is_some(),
        "volume scan complete"
    );

    let config = hyprdrive_object_pipeline::PipelineConfig::new(volume_id);
    let pipeline = hyprdrive_object_pipeline::ObjectPipeline::new_shared(
        config,
        pool.clone(),
        Arc::clone(cache),
    );
    let stats = pipeline
        .process_entries(&result.entries)
        .await
        .context("object pipeline failed")?;
    info!(
        total = stats.total,
        hashed = stats.hashed,
        cached = stats.cached,
        skipped = stats.skipped,
        errors = stats.errors,
        directories = stats.directories,
        zero_byte = stats.zero_byte,
        elapsed_ms = stats.elapsed.as_millis() as u64,
        "object pipeline complete"
    );

    Ok(result)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize structured logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(true)
        .with_thread_ids(true)
        .init();

    info!("HyprDrive daemon starting...");
    info!("Version: {}", env!("CARGO_PKG_VERSION"));

    // ── Phase 2: Database ──
    let data_dir = dirs::data_dir()
        .context("could not determine platform data directory")?
        .join("HyprDrive");
    std::fs::create_dir_all(&data_dir)?;

    let db_path = data_dir.join("hyprdrive.db");
    info!(path = %db_path.display(), "opening database");
    let pool = hyprdrive_core::db::pool::create_pool(&db_path).await?;
    hyprdrive_core::db::pool::run_migrations(&pool).await?;
    info!("database ready");

    let cache_path = data_dir.join("cache.redb");
    #[cfg_attr(target_os = "macos", allow(unused_variables))]
    let cache = Arc::new(
        hyprdrive_core::db::cache::open_cache(&cache_path).context("failed to open redb cache")?,
    );
    info!(path = %cache_path.display(), "redb hot-cache ready");

    // Channel for rescan requests from watcher.
    #[cfg_attr(target_os = "macos", allow(unused_variables))]
    let (rescan_tx, mut rescan_rx) = tokio::sync::mpsc::channel::<std::path::PathBuf>(4);

    // Track watcher task and listener for shutdown.
    let mut _watcher_task: Option<tokio::task::JoinHandle<()>> = None;

    // ── Phase 3: Volume scanning + Phase 8: Real-time watcher ──
    // Windows: NTFS MFT scan → USN journal listener
    #[cfg(target_os = "windows")]
    let mut _usn_listener: Option<hyprdrive_fs_indexer::UsnListener> = None;

    #[cfg(target_os = "windows")]
    {
        let scan_root = std::path::Path::new(DEFAULT_SCAN_ROOT);
        match run_full_scan(scan_root, &pool, &cache).await {
            Ok(result) => {
                let volume_id = derive_volume_id(scan_root);

                // ── Phase 8: Real-time watcher ──
                if result.cursor.is_some() {
                    // 1. Create cursor store and pre-seed with scan cursor.
                    let cursor_store = Arc::new(cursor_store::SqliteCursorStore::new(pool.clone()));
                    if let Some(ref c) = result.cursor {
                        let store = Arc::clone(&cursor_store);
                        let vol = volume_id.clone();
                        let cursor_json =
                            serde_json::to_string(c).expect("UsnCursor serialization cannot fail");
                        if let Err(e) = tokio::task::spawn_blocking(move || {
                            use hyprdrive_fs_indexer::CursorStore;
                            store.save(&vol, &cursor_json)
                        })
                        .await?
                        {
                            tracing::warn!(error = %e, "failed to pre-seed cursor");
                        }
                    }

                    // 2. Create change processor and seed fid map from initial scan.
                    let processor = Arc::new(hyprdrive_object_pipeline::ChangeProcessor::new(
                        volume_id.clone(),
                        pool.clone(),
                        Arc::clone(&cache),
                    ));
                    processor.seed_fid_map(&result.entries);
                    info!(
                        fid_map_entries = result.entries.len(),
                        "change processor fid map seeded"
                    );

                    // 3. Start USN listener.
                    let listener_config = hyprdrive_fs_indexer::ListenerConfig {
                        volumes: vec![scan_root.to_path_buf()],
                        ..Default::default()
                    };
                    let (usn_listener, rx) =
                        hyprdrive_fs_indexer::UsnListener::new(listener_config);
                    match usn_listener.start(cursor_store) {
                        Ok(_handles) => {
                            info!("real-time watcher started (USN journal)");

                            // 4. Spawn watcher loop.
                            let mut wloop =
                                watcher::WatcherLoop::new(rx, processor, rescan_tx.clone());
                            _watcher_task = Some(tokio::spawn(async move { wloop.run().await }));
                            _usn_listener = Some(usn_listener);
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "failed to start USN listener — real-time watching disabled");
                        }
                    }
                } else {
                    info!("no USN cursor available — real-time watching disabled (fallback scan was used)");
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "volume scan failed — will retry on next cycle");
                for cause in e.chain().skip(1) {
                    tracing::warn!(cause = %cause, "  caused by");
                }
            }
        }
    }

    // Linux: jwalk scan → inotify listener
    #[cfg(target_os = "linux")]
    let mut _linux_listener: Option<hyprdrive_fs_indexer::LinuxListener> = None;

    #[cfg(target_os = "linux")]
    {
        let scan_root = default_scan_root();
        match run_full_scan(&scan_root, &pool, &cache).await {
            Ok(result) => {
                let volume_id = derive_volume_id(&scan_root);

                // Pre-seed linux cursor if available.
                if let Some(ref c) = result.linux_cursor {
                    let cursor_store = Arc::new(cursor_store::SqliteCursorStore::new(pool.clone()));
                    let store = Arc::clone(&cursor_store);
                    let vol = volume_id.clone();
                    let cursor_json =
                        serde_json::to_string(c).expect("LinuxCursor serialization cannot fail");
                    if let Err(e) = tokio::task::spawn_blocking(move || {
                        use hyprdrive_fs_indexer::CursorStore;
                        store.save(&vol, &cursor_json)
                    })
                    .await?
                    {
                        tracing::warn!(error = %e, "failed to pre-seed linux cursor");
                    }
                }

                // Create change processor and seed fid map from initial scan.
                let processor = Arc::new(hyprdrive_object_pipeline::ChangeProcessor::new(
                    volume_id,
                    pool.clone(),
                    Arc::clone(&cache),
                ));
                processor.seed_fid_map(&result.entries);
                info!(
                    fid_map_entries = result.entries.len(),
                    "change processor fid map seeded"
                );

                // Start inotify listener.
                let listener_config = hyprdrive_fs_indexer::LinuxListenerConfig {
                    root: scan_root.clone(),
                    ..Default::default()
                };
                let (linux_listener, rx) =
                    hyprdrive_fs_indexer::LinuxListener::new(listener_config);
                match linux_listener.start() {
                    Ok(_handle) => {
                        info!("real-time watcher started (inotify)");

                        // Spawn watcher loop.
                        let mut wloop = watcher::WatcherLoop::new(rx, processor, rescan_tx.clone());
                        _watcher_task = Some(tokio::spawn(async move { wloop.run().await }));
                        _linux_listener = Some(linux_listener);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to start inotify listener — real-time watching disabled");
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "volume scan failed — will retry on next cycle");
            }
        }
    }

    // FIXME(phase-9): start EventBus (tokio::broadcast channel for domain events)
    // FIXME(phase-13): start Iroh P2P node for device sync
    // FIXME(phase-13): start Axum HTTP server on :7421 for UI/CLI clients

    // ── Event loop: handle rescans and shutdown ──
    info!("Daemon ready. Press Ctrl+C to stop.");
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Shutdown signal received.");
                break;
            }
            result = rescan_rx.recv() => {
                match result {
                    Some(volume) => {
                        info!(volume = %volume.display(), "Full rescan requested by watcher");
                        #[cfg(any(target_os = "windows", target_os = "linux"))]
                        {
                            // Determine scan root for rescan.
                            #[cfg(target_os = "windows")]
                            let scan_root = if volume.as_os_str().is_empty() {
                                std::path::PathBuf::from(DEFAULT_SCAN_ROOT)
                            } else {
                                volume
                            };
                            #[cfg(target_os = "linux")]
                            let scan_root = if volume.as_os_str().is_empty() {
                                default_scan_root()
                            } else {
                                volume
                            };

                            if let Err(e) = run_full_scan(&scan_root, &pool, &cache).await {
                                tracing::error!(error = %e, "rescan failed");
                            }
                        }
                        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
                        {
                            tracing::warn!(volume = %volume.display(), "rescan not supported on this platform");
                        }
                    }
                    None => {
                        info!("Rescan channel closed — shutting down.");
                        break;
                    }
                }
            }
        }
    }

    // Shut down watcher.
    #[cfg(target_os = "windows")]
    {
        if let Some(l) = _usn_listener.take() {
            l.shutdown();
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(l) = _linux_listener.take() {
            l.shutdown();
        }
    }
    if let Some(t) = _watcher_task.take() {
        match tokio::time::timeout(std::time::Duration::from_secs(10), t).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::error!(error = %e, "watcher task panicked"),
            Err(_) => tracing::warn!("watcher task did not stop within 10s"),
        }
    }

    pool.close().await;
    info!("HyprDrive daemon stopped.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn smoke() {
        // Placeholder — ensures this crate appears in `cargo test` output.
    }

    #[test]
    fn derive_volume_id_windows_drive() {
        assert_eq!(derive_volume_id(Path::new("C:\\")), "C");
        assert_eq!(derive_volume_id(Path::new("D:\\")), "D");
    }

    #[test]
    fn derive_volume_id_linux_path() {
        assert_eq!(derive_volume_id(Path::new("/home")), "home");
        assert_eq!(derive_volume_id(Path::new("/home/user")), "user");
    }

    #[test]
    fn derive_volume_id_root() {
        assert_eq!(derive_volume_id(Path::new("/")), "root");
    }
}
