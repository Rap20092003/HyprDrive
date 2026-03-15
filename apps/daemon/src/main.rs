//! HyprDrive Daemon — The System
//!
//! This is THE primary binary. All UIs are thin clients that connect here.
//! The daemon owns: database, indexing, sync, crypto, extensions, and HTTP API.

use anyhow::{Context, Result};
use tracing::info;

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
    let _cache = hyprdrive_core::db::cache::open_cache(&cache_path)
        .context("failed to open redb cache")?;
    info!(path = %cache_path.display(), "redb hot-cache ready");

    // ── Phase 3: Volume scanning (stub) ──
    // Auto-detect filesystem and scan using the best available strategy.
    // MFT scan requires admin — falls back to jwalk automatically.
    #[cfg(target_os = "windows")]
    {
        info!("starting volume scan...");
        match hyprdrive_fs_indexer::auto_scan(std::path::Path::new("C:\\")) {
            Ok(result) => {
                info!(
                    entries = result.entries.len(),
                    has_cursor = result.cursor.is_some(),
                    "volume scan complete"
                );
                // TODO Phase 7: hash entries → insert into objects table via pool
                // TODO Phase 8: compute disk intelligence (treemap, dir sizes)
            }
            Err(e) => {
                tracing::warn!(error = %e, "volume scan failed — will retry on next cycle");
            }
        }
    }

    // TODO Phase 9: Start EventBus
    // TODO Phase 10: Start file watchers
    // TODO Phase 13: Start Iroh P2P node
    // TODO Phase 13: Start Axum HTTP server on :7421

    // Graceful shutdown: wait for Ctrl+C
    info!("Daemon ready. Press Ctrl+C to stop.");
    tokio::signal::ctrl_c().await?;
    info!("Shutdown signal received. Cleaning up...");

    pool.close().await;
    info!("HyprDrive daemon stopped.");
    Ok(())
}
