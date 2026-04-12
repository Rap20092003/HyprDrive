//! Read-only query endpoints.
//!
//! All endpoints under `/queries/*` are side-effect-free and
//! return JSON-serialized results from the database.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Deserialize;

use hyprdrive_core::db::queries;

use super::AppState;

/// Build the `/queries` sub-router.
pub fn mount() -> Router<AppState> {
    Router::new()
        .route("/volume_summary", get(volume_summary))
        .route("/top_largest_files", get(top_largest_files))
        .route("/duplicates", get(duplicates))
        .route("/type_breakdown", get(type_breakdown))
        .route("/stale", get(stale))
        .route("/tags_for_object", get(tags_for_object))
        .route("/search", get(search))
}

// ── Query parameter types ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct VolumeParams {
    volume_id: String,
}

#[derive(Deserialize)]
struct TopParams {
    volume_id: String,
    #[serde(default = "default_limit")]
    limit: i64,
}

#[derive(Deserialize)]
struct StaleParams {
    volume_id: String,
    #[serde(default = "default_stale_days")]
    days: i64,
    #[serde(default = "default_limit")]
    limit: i64,
}

#[derive(Deserialize)]
struct ObjectParams {
    object_id: String,
}

#[derive(Deserialize)]
struct SearchParams {
    q: String,
    #[serde(default = "default_limit")]
    limit: i64,
}

const fn default_limit() -> i64 {
    50
}
const fn default_stale_days() -> i64 {
    365
}

// ── Handlers ─────────────────────────────────────────────────────────────────

async fn volume_summary(
    State(ctx): State<AppState>,
    Query(params): Query<VolumeParams>,
) -> impl IntoResponse {
    match queries::volume_summary(&ctx.index.pool, &params.volume_id).await {
        Ok(summary) => Json(summary).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("DB error: {e}"),
        )
            .into_response(),
    }
}

async fn top_largest_files(
    State(ctx): State<AppState>,
    Query(params): Query<TopParams>,
) -> impl IntoResponse {
    match queries::top_largest_files(&ctx.index.pool, &params.volume_id, params.limit).await {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")).into_response(),
    }
}

async fn duplicates(
    State(ctx): State<AppState>,
    Query(params): Query<TopParams>,
) -> impl IntoResponse {
    match queries::duplicates_report(&ctx.index.pool, &params.volume_id, params.limit).await {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")).into_response(),
    }
}

async fn type_breakdown(
    State(ctx): State<AppState>,
    Query(params): Query<VolumeParams>,
) -> impl IntoResponse {
    match queries::type_breakdown(&ctx.index.pool, &params.volume_id).await {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")).into_response(),
    }
}

async fn stale(
    State(ctx): State<AppState>,
    Query(params): Query<StaleParams>,
) -> impl IntoResponse {
    match queries::stale_files(&ctx.index.pool, &params.volume_id, params.days, params.limit).await
    {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")).into_response(),
    }
}

async fn tags_for_object(
    State(ctx): State<AppState>,
    Query(params): Query<ObjectParams>,
) -> impl IntoResponse {
    match queries::tags_for_object(&ctx.index.pool, &params.object_id).await {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")).into_response(),
    }
}

async fn search(
    State(ctx): State<AppState>,
    Query(params): Query<SearchParams>,
) -> impl IntoResponse {
    match queries::search_files(&ctx.index.pool, &params.q, params.limit).await {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")).into_response(),
    }
}
