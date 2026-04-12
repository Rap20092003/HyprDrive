//! Axum HTTP router for the HyprDrive daemon.
//!
//! Exposes a JSON API on `:7421`:
//! - `GET  /health`               — liveness check
//! - `GET  /queries/{endpoint}`   — read-only queries (files, stats, tags)
//! - `POST /actions/{action}`     — mutating file-system commands with undo

pub mod mutations;
pub mod queries;

use axum::{routing::get, Router};
use std::sync::Arc;

use hyprdrive_core::ops::OperationsContext;

/// Shared application state threaded through all route handlers.
pub type AppState = Arc<OperationsContext>;

/// Build the complete axum router.
pub fn build_router(ctx: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .nest("/queries", queries::mount())
        .nest("/actions", mutations::mount())
        .with_state(ctx)
}

/// `GET /health` — returns `"ok"` if the daemon is alive.
async fn health() -> &'static str {
    "ok"
}
