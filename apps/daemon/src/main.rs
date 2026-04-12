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

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod cursor_store;
mod router;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod watcher;

use anyhow::{Context, Result};
#[cfg(target_os = "windows")]
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

/// Default scan root for Linux — user's home directory.
#[cfg(target_os = "linux")]
fn default_scan_root() -> std::path::PathBuf {
    dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/home"))
}

/// Derive a volume ID from a scan root path.
///
/// - Windows drive letters: `"C"` from `"C:\\"`
/// - WSL UNC paths: `"wsl:Ubuntu"` from `"\\wsl.localhost\Ubuntu\"`
/// - Linux/macOS: last path component (e.g. `"home"` from `"/home"`)
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
    // WSL UNC paths: "wsl:Ubuntu" from "\\wsl.localhost\Ubuntu\"
    if lossy.starts_with("\\\\wsl.localhost\\") || lossy.starts_with("\\\\wsl$\\") {
        let prefix = if lossy.starts_with("\\\\wsl.localhost\\") {
            "\\\\wsl.localhost\\"
        } else {
            "\\\\wsl$\\"
        };
        let rest = &lossy[prefix.len()..];
        let name = rest.split('\\').next().unwrap_or("unknown");
        return format!("wsl:{}", name);
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
    let volume_id_for_intel = volume_id.clone();
    info!(
        entries = result.entries.len(),
        has_usn_cursor = result.cursor.is_some(),
        has_linux_cursor = result.linux_cursor.is_some(),
        "volume scan complete"
    );

    let mut config = hyprdrive_object_pipeline::PipelineConfig::new(volume_id);
    config.defer_content_hashing = true;
    let pipeline = hyprdrive_object_pipeline::ObjectPipeline::new_shared(
        config,
        pool.clone(),
        Arc::clone(cache),
    );

    // Bulk load mode: drop FTS triggers + synchronous=OFF for 10-20x speedup.
    hyprdrive_core::db::queries::bulk_load_begin(pool)
        .await
        .context("bulk_load_begin failed")?;
    let stats = pipeline
        .process_entries(&result.entries)
        .await
        .context("object pipeline failed")?;
    // Rebuild FTS index in one pass and restore triggers.
    hyprdrive_core::db::queries::bulk_load_finish(pool)
        .await
        .context("bulk_load_finish (FTS rebuild) failed")?;

    // Populate directory size aggregations for disk intelligence.
    hyprdrive_core::db::queries::populate_dir_sizes(pool, &volume_id_for_intel)
        .await
        .context("dir_sizes population failed")?;

    // Log disk intelligence summary.
    let summary = hyprdrive_core::db::queries::volume_summary(pool, &volume_id_for_intel)
        .await
        .context("volume summary failed")?;
    info!(
        files = summary.total_files,
        dirs = summary.total_dirs,
        total_bytes = summary.total_bytes,
        allocated_bytes = summary.total_allocated,
        wasted_bytes = summary.wasted_bytes,
        "disk intelligence summary"
    );

    // Populate redb DIR_SIZE_CACHE from SQLite dir_sizes for fast lookups.
    {
        let dir_rows: Vec<hyprdrive_core::db::types::DirSizeRow> = sqlx::query_as(
            "SELECT location_id, file_count, total_bytes, allocated_bytes, cumulative_allocated
             FROM dir_sizes
             WHERE location_id IN (SELECT id FROM locations WHERE volume_id = ?1)",
        )
        .bind(&volume_id_for_intel)
        .fetch_all(pool)
        .await
        .context("fetch dir_sizes for cache")?;

        let entries: Vec<(String, hyprdrive_core::db::cache::DirSizeRecord)> = dir_rows
            .iter()
            .map(|r| {
                (
                    r.location_id.clone(),
                    hyprdrive_core::db::cache::DirSizeRecord {
                        file_count: r.file_count as u64,
                        total_bytes: r.total_bytes as u64,
                        cumulative_allocated: r.cumulative_allocated as u64,
                    },
                )
            })
            .collect();

        if !entries.is_empty() {
            hyprdrive_core::db::cache::dir_size::populate_batch(cache, &entries)
                .context("DIR_SIZE_CACHE population failed")?;
            info!(entries = entries.len(), "DIR_SIZE_CACHE populated");
        }
    }

    info!(
        total = stats.total,
        hashed = stats.hashed,
        cached = stats.cached,
        deferred = stats.deferred,
        skipped = stats.skipped,
        errors = stats.errors,
        directories = stats.directories,
        zero_byte = stats.zero_byte,
        elapsed_ms = stats.elapsed.as_millis() as u64,
        "object pipeline complete"
    );

    // Log pending deferred hash count.
    let pending = hyprdrive_core::db::queries::pending_hash_count(pool)
        .await
        .unwrap_or(0);
    if pending > 0 {
        info!(pending, "deferred objects awaiting background hashing");
    }

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

    // Default volume for HTTP API (set during scan, used for OperationsContext).
    #[allow(unused_mut)]
    let mut default_volume_id = String::from("C");

    // ── Phase 3: Multi-volume scanning + Phase 8: Real-time watcher ──
    // Windows: discover all volumes → parallel MFT/jwalk scan → USN journal listener
    #[cfg(target_os = "windows")]
    let mut _usn_listener: Option<hyprdrive_fs_indexer::UsnListener> = None;

    #[cfg(target_os = "windows")]
    {
        // ── Step 1: Discover all volumes ──
        let volumes = hyprdrive_fs_indexer::enumerate_volumes();
        info!(
            count = volumes.len(),
            "discovered volumes: {}",
            volumes
                .iter()
                .map(|d| format!("{}({:?})", d.volume_id, d.drive_type))
                .collect::<Vec<_>>()
                .join(", ")
        );

        if volumes.is_empty() {
            tracing::warn!("no scannable volumes found");
        }

        // ── Step 2: Scan all volumes in parallel ──
        let mut scan_set = tokio::task::JoinSet::new();
        for drive in &volumes {
            let path = drive.path.clone();
            let pool = pool.clone();
            let cache = Arc::clone(&cache);
            scan_set.spawn(async move {
                let volume_id = derive_volume_id(&path);
                match run_full_scan(&path, &pool, &cache).await {
                    Ok(result) => Some((path, volume_id, result)),
                    Err(e) => {
                        tracing::warn!(
                            volume = %path.display(),
                            error = %e,
                            "scan failed for volume — skipping"
                        );
                        for cause in e.chain().skip(1) {
                            tracing::warn!(cause = %cause, "  caused by");
                        }
                        None
                    }
                }
            });
        }

        // Collect results from all parallel scans.
        let mut scan_results: Vec<(std::path::PathBuf, String, hyprdrive_fs_indexer::ScanResult)> =
            Vec::new();
        while let Some(result) = scan_set.join_next().await {
            match result {
                Ok(Some(tuple)) => scan_results.push(tuple),
                Ok(None) => {} // scan failed, already logged
                Err(e) => tracing::error!(error = %e, "scan task panicked"),
            }
        }

        info!(
            successful = scan_results.len(),
            total = volumes.len(),
            "volume scans complete"
        );

        // Set default volume for HTTP API from first successfully scanned volume.
        if let Some((_, vid, _)) = scan_results.first() {
            default_volume_id = vid.clone();
        }

        // ── Step 3: Build per-volume ChangeProcessors ──
        let cursor_store = Arc::new(cursor_store::SqliteCursorStore::new(pool.clone()));
        let mut processors: HashMap<String, Arc<hyprdrive_object_pipeline::ChangeProcessor>> =
            HashMap::new();
        let mut ntfs_volumes: Vec<std::path::PathBuf> = Vec::new();

        for (path, volume_id, result) in &scan_results {
            // Pre-seed USN cursor if available (NTFS volumes only).
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
                    tracing::warn!(volume = %volume_id, error = %e, "failed to pre-seed cursor");
                }
                ntfs_volumes.push(path.clone());
            }

            // Create processor and seed fid_map.
            let processor = Arc::new(hyprdrive_object_pipeline::ChangeProcessor::new(
                volume_id.clone(),
                pool.clone(),
                Arc::clone(&cache),
            ));
            processor.seed_fid_map(&result.entries);
            info!(
                volume = %volume_id,
                fid_map_entries = result.entries.len(),
                "change processor seeded"
            );
            processors.insert(volume_id.clone(), processor);
        }

        // ── Step 4: Start USN listener for NTFS volumes ──
        if !ntfs_volumes.is_empty() {
            let mut listener_config = hyprdrive_fs_indexer::ListenerConfig::default();
            for vol in &ntfs_volumes {
                listener_config = listener_config.add_volume(vol.clone());
            }
            // Scale channel capacity with volume count.
            listener_config = listener_config.with_capacity(10_000 * ntfs_volumes.len());

            let (usn_listener, rx) = hyprdrive_fs_indexer::UsnListener::new(listener_config);
            match usn_listener.start(cursor_store) {
                Ok(_handles) => {
                    info!(
                        volumes = ntfs_volumes.len(),
                        "real-time watcher started (USN journal)"
                    );

                    // ── Step 5: Spawn watcher loop with all processors ──
                    let mut wloop =
                        watcher::WatcherLoop::new_multi(rx, processors, rescan_tx.clone());
                    _watcher_task = Some(tokio::spawn(async move { wloop.run().await }));
                    _usn_listener = Some(usn_listener);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to start USN listener");
                }
            }
        } else {
            info!("no NTFS volumes found — real-time watching disabled");
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
                default_volume_id = volume_id.clone();

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
                let (linux_listener, _rx) =
                    hyprdrive_fs_indexer::LinuxListener::new(listener_config);
                match linux_listener.start() {
                    Ok(_handle) => {
                        info!("real-time watcher started (inotify)");
                        // NOTE: Linux watcher loop needs VolumedChange migration too.
                        // For now, linux real-time watching is disabled until
                        // LinuxListener emits VolumedChange.
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

    // ── Background hasher: upgrade deferred objects to real BLAKE3 hashes ──
    let bg_cancel = tokio_util::sync::CancellationToken::new();
    let pending = match hyprdrive_core::db::queries::pending_hash_count(&pool).await {
        Ok(n) => n,
        Err(e) => {
            tracing::warn!(error = %e, "pending_hash_count failed, skipping background hasher");
            0
        }
    };
    let mut bg_hasher_task: Option<tokio::task::JoinHandle<()>> = if pending > 0 {
        info!(pending, "spawning background hasher");
        // Use "multi" as volume_id since deferred objects span multiple volumes.
        // The hasher derives per-row volume_id from the DB query.
        let volume_id = "multi".to_string();

        let bg_config = hyprdrive_object_pipeline::BackgroundHasherConfig::new(volume_id);
        let bg_pool = pool.clone();
        let bg_cache = Arc::clone(&cache);
        let bg_token = bg_cancel.clone();
        Some(tokio::spawn(async move {
            let result = hyprdrive_object_pipeline::run_background_hasher(
                bg_config, bg_pool, bg_cache, bg_token,
            )
            .await;
            info!(
                upgraded = result.upgraded,
                errors = result.errors,
                "background hasher finished"
            );
        }))
    } else {
        None
    };

    // FIXME(phase-13): start Iroh P2P node for device sync

    // ── Phase 9: start Axum HTTP server on :7421 ──
    let ops_ctx = {
        use hyprdrive_core::domain::id::DeviceId;
        use hyprdrive_core::domain::undo::UndoStack;
        use hyprdrive_core::ops::{
            IndexContext, OperationsContext, SessionContext, StorageContext,
        };
        use tokio::sync::Mutex;

        Arc::new(OperationsContext {
            session: SessionContext {
                device_id: DeviceId::new(),
                permissions: vec!["read".into(), "write".into(), "delete".into()],
                source: "daemon".into(),
                correlation_id: None,
            },
            storage: StorageContext {
                volume_id: default_volume_id.clone(),
            },
            index: IndexContext {
                pool: pool.clone(),
                cache: Arc::clone(&cache),
            },
            undo_stack: Arc::new(Mutex::new(UndoStack::new())),
        })
    };
    let http_router = {
        use tower_http::cors::{Any, CorsLayer};
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);
        router::build_router(ops_ctx).layer(cors)
    };
    let http_listener = tokio::net::TcpListener::bind("0.0.0.0:7421")
        .await
        .context("failed to bind HTTP server on :7421")?;
    info!("HTTP API listening on http://0.0.0.0:7421");
    tokio::spawn(async move {
        if let Err(e) = axum::serve(http_listener, http_router).await {
            tracing::error!(error = %e, "HTTP server error");
        }
    });

    // ── Event loop: handle rescans and shutdown ──
    info!("Daemon ready. Press Ctrl+C to stop.");
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Shutdown signal received.");
                bg_cancel.cancel();
                break;
            }
            result = rescan_rx.recv() => {
                match result {
                    Some(volume) => {
                        info!(volume = %volume.display(), "Full rescan requested by watcher");

                        // M4: Cancel background hasher before rescan to avoid
                        // concurrent writes during bulk_load_begin (synchronous=OFF).
                        bg_cancel.cancel();
                        if let Some(t) = bg_hasher_task.take() {
                            match tokio::time::timeout(std::time::Duration::from_secs(30), t).await {
                                Ok(Ok(())) => {}
                                Ok(Err(e)) => tracing::error!(error = %e, "background hasher panicked"),
                                Err(_) => tracing::warn!("background hasher did not stop within 30s for rescan"),
                            }
                        }

                        #[cfg(any(target_os = "windows", target_os = "linux"))]
                        {
                            // Determine scan root for rescan.
                            #[cfg(target_os = "windows")]
                            let scan_root = if volume.as_os_str().is_empty() {
                                // Default: rescan C: as fallback. Full multi-rescan is future work.
                                std::path::PathBuf::from("C:\\")
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

    // H3: Await background hasher with timeout before closing pool.
    if let Some(t) = bg_hasher_task.take() {
        match tokio::time::timeout(std::time::Duration::from_secs(30), t).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::error!(error = %e, "background hasher panicked"),
            Err(_) => tracing::warn!("background hasher did not stop within 30s"),
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

    #[test]
    fn derive_volume_id_wsl_localhost() {
        assert_eq!(
            derive_volume_id(Path::new("\\\\wsl.localhost\\Ubuntu\\")),
            "wsl:Ubuntu"
        );
    }

    #[test]
    fn derive_volume_id_wsl_dollar() {
        assert_eq!(
            derive_volume_id(Path::new("\\\\wsl$\\Debian\\")),
            "wsl:Debian"
        );
    }
}
