//! HyprDrive Daemon — The System
//!
//! This is THE primary binary. All UIs are thin clients that connect here.
//! The daemon owns: database, indexing, sync, crypto, extensions, and HTTP API.

use anyhow::Result;
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

    // TODO Phase 2: Initialize SQLite database
    // TODO Phase 9: Start EventBus
    // TODO Phase 10: Start file watchers
    // TODO Phase 13: Start Iroh P2P node
    // TODO Phase 13: Start Axum HTTP server on :7421

    // Graceful shutdown: wait for Ctrl+C
    info!("Daemon ready. Press Ctrl+C to stop.");
    tokio::signal::ctrl_c().await?;
    info!("Shutdown signal received. Cleaning up...");

    // TODO: Graceful service shutdown

    info!("HyprDrive daemon stopped.");
    Ok(())
}
