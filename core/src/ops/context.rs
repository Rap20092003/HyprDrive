//! Execution context types passed to every [`super::CoreAction`].

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::domain::id::DeviceId;
use crate::domain::undo::{UndoEntry, UndoStack};

// ── SessionContext ──────────────────────────────────────────────────────────

/// Who is calling the action and with what authority.
#[derive(Debug, Clone)]
pub struct SessionContext {
    /// Identity of the device/agent executing the action.
    pub device_id: DeviceId,
    /// Capabilities granted to this session (e.g. `["read", "write", "delete"]`).
    pub permissions: Vec<String>,
    /// Originating subsystem: `"desktop-ui"`, `"cli"`, `"sync-engine"`.
    pub source: String,
    /// Optional correlation ID for distributed tracing across services.
    pub correlation_id: Option<String>,
}

// ── StorageContext ──────────────────────────────────────────────────────────

/// Which volume the action should target.
#[derive(Debug, Clone)]
pub struct StorageContext {
    /// Short volume identifier used for inode cache keys and DB foreign keys.
    /// Examples: `"C"`, `"D"`, `"wsl:Ubuntu"`.
    pub volume_id: String,
}

// ── IndexContext ────────────────────────────────────────────────────────────

/// Handles to the persistence layer consumed by action implementations.
#[derive(Clone)]
pub struct IndexContext {
    /// Shared SQLite connection pool (metadata store).
    pub pool: sqlx::SqlitePool,
    /// Hot inode / directory-size cache (redb).
    pub cache: Arc<redb::Database>,
}

impl std::fmt::Debug for IndexContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IndexContext")
            .field("pool", &"SqlitePool")
            .field("cache", &"redb::Database")
            .finish()
    }
}

// ── OperationsContext ───────────────────────────────────────────────────────

/// Top-level context bundle passed into every action's `execute()`.
#[derive(Clone)]
pub struct OperationsContext {
    /// Who is performing the action.
    pub session: SessionContext,
    /// Which volume to target.
    pub storage: StorageContext,
    /// Persistence handles.
    pub index: IndexContext,
    /// Shared undo stack for all actions in this session.
    pub undo_stack: Arc<Mutex<UndoStack>>,
}

impl std::fmt::Debug for OperationsContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OperationsContext")
            .field("session", &self.session)
            .field("storage", &self.storage)
            .field("index", &self.index)
            .finish()
    }
}

impl OperationsContext {
    /// Push an undo entry onto the shared stack.
    pub async fn push_undo(&self, entry: UndoEntry) {
        let mut stack = self.undo_stack.lock().await;
        stack.push(entry);
    }

    /// Pop the most recent undo entry from the shared stack.
    pub async fn pop_undo(&self) -> Option<UndoEntry> {
        let mut stack = self.undo_stack.lock().await;
        stack.pop()
    }
}
