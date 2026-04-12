//! Mutation endpoints (POST) that execute CQRS actions.
//!
//! Each endpoint deserializes a JSON body into the action's `Input`,
//! calls `execute()`, pushes the undo entry, and returns the `Output`.

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use serde_json::Value;

use hyprdrive_core::domain::undo::UndoEntry;
use hyprdrive_core::ops::actions::{
    bulk_tag, copy_file, create_dir, empty_trash, move_file, rename, smart_rename, soft_delete,
};
use hyprdrive_core::ops::CoreAction;

use super::AppState;

/// Build the `/actions` sub-router.
pub fn mount() -> Router<AppState> {
    Router::new()
        .route("/create_dir", post(action_create_dir))
        .route("/rename", post(action_rename))
        .route("/copy_file", post(action_copy_file))
        .route("/move_file", post(action_move_file))
        .route("/soft_delete", post(action_soft_delete))
        .route("/bulk_tag", post(action_bulk_tag))
        .route("/empty_trash", post(action_empty_trash))
        .route("/smart_rename", post(action_smart_rename))
        .route("/undo", post(action_undo))
        .route("/registry", post(action_registry))
}

// ── Generic action runner ────────────────────────────────────────────────────

/// Execute an action, push the undo entry, return output + undo description.
async fn run_action<A: CoreAction>(
    action: &A,
    ctx: &AppState,
    input: A::Input,
) -> Result<(A::Output, String), (StatusCode, String)> {
    let (output, undo_entry) = action
        .execute(ctx, input)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("{e}")))?;
    let desc = undo_entry.description.clone();
    ctx.push_undo(undo_entry).await;
    Ok((output, desc))
}

// ── Action handlers ──────────────────────────────────────────────────────────

async fn action_create_dir(
    State(ctx): State<AppState>,
    Json(input): Json<create_dir::CreateDirInput>,
) -> impl IntoResponse {
    match run_action(&create_dir::CreateDir, &ctx, input).await {
        Ok((output, _)) => Json(serde_json::to_value(output).unwrap_or(Value::Null)).into_response(),
        Err((status, msg)) => (status, msg).into_response(),
    }
}

async fn action_rename(
    State(ctx): State<AppState>,
    Json(input): Json<rename::RenameInput>,
) -> impl IntoResponse {
    match run_action(&rename::Rename, &ctx, input).await {
        Ok((output, _)) => Json(serde_json::to_value(output).unwrap_or(Value::Null)).into_response(),
        Err((status, msg)) => (status, msg).into_response(),
    }
}

async fn action_copy_file(
    State(ctx): State<AppState>,
    Json(input): Json<copy_file::CopyFileInput>,
) -> impl IntoResponse {
    match run_action(&copy_file::CopyFile, &ctx, input).await {
        Ok((output, _)) => Json(serde_json::to_value(output).unwrap_or(Value::Null)).into_response(),
        Err((status, msg)) => (status, msg).into_response(),
    }
}

async fn action_move_file(
    State(ctx): State<AppState>,
    Json(input): Json<move_file::MoveFileInput>,
) -> impl IntoResponse {
    match run_action(&move_file::MoveFile, &ctx, input).await {
        Ok((output, _)) => Json(serde_json::to_value(output).unwrap_or(Value::Null)).into_response(),
        Err((status, msg)) => (status, msg).into_response(),
    }
}

async fn action_soft_delete(
    State(ctx): State<AppState>,
    Json(input): Json<soft_delete::SoftDeleteInput>,
) -> impl IntoResponse {
    match run_action(&soft_delete::SoftDelete, &ctx, input).await {
        Ok((output, _)) => Json(serde_json::to_value(output).unwrap_or(Value::Null)).into_response(),
        Err((status, msg)) => (status, msg).into_response(),
    }
}

async fn action_bulk_tag(
    State(ctx): State<AppState>,
    Json(input): Json<bulk_tag::BulkTagInput>,
) -> impl IntoResponse {
    match run_action(&bulk_tag::BulkTag, &ctx, input).await {
        Ok((output, _)) => Json(serde_json::to_value(output).unwrap_or(Value::Null)).into_response(),
        Err((status, msg)) => (status, msg).into_response(),
    }
}

async fn action_empty_trash(
    State(ctx): State<AppState>,
    Json(input): Json<empty_trash::EmptyTrashInput>,
) -> impl IntoResponse {
    match run_action(&empty_trash::EmptyTrash, &ctx, input).await {
        Ok((output, _)) => Json(serde_json::to_value(output).unwrap_or(Value::Null)).into_response(),
        Err((status, msg)) => (status, msg).into_response(),
    }
}

async fn action_smart_rename(
    State(ctx): State<AppState>,
    Json(input): Json<smart_rename::SmartRenameInput>,
) -> impl IntoResponse {
    match run_action(&smart_rename::SmartRename, &ctx, input).await {
        Ok((output, _)) => Json(serde_json::to_value(output).unwrap_or(Value::Null)).into_response(),
        Err((status, msg)) => (status, msg).into_response(),
    }
}

// ── Undo endpoint ────────────────────────────────────────────────────────────

/// `POST /actions/undo` — pop the last undo entry and return its inverse JSON.
async fn action_undo(State(ctx): State<AppState>) -> impl IntoResponse {
    match ctx.pop_undo().await {
        Some(UndoEntry {
            description,
            inverse_action,
            ..
        }) => Json(serde_json::json!({
            "undone": description,
            "inverse_action": serde_json::from_str::<Value>(&inverse_action)
                .unwrap_or(Value::String(inverse_action)),
        }))
        .into_response(),
        None => (StatusCode::NOT_FOUND, "Nothing to undo").into_response(),
    }
}

// ── Registry listing ─────────────────────────────────────────────────────────

/// `POST /actions/registry` — list all registered actions and their metadata.
async fn action_registry() -> impl IntoResponse {
    let registry = hyprdrive_core::ops::ActionRegistry::build();
    let actions: Vec<Value> = registry
        .list()
        .into_iter()
        .map(|name| {
            let meta = registry.get(name);
            serde_json::json!({
                "name": name,
                "description": meta.map(|m| m.description).unwrap_or(""),
                "undoable": meta.map(|m| m.undoable).unwrap_or(false),
            })
        })
        .collect();
    Json(serde_json::json!({ "actions": actions }))
}
