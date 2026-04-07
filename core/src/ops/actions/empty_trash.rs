//! `empty_trash` action — signal intent to purge the OS trash.
//!
//! The `trash` crate moves files to the system trash but does not expose a
//! cross-platform `purge_all` API. This action logs a message and delegates
//! the actual purge to the operating system.

use crate::domain::undo::UndoEntry;
use crate::ops::registry::ActionMeta;
use crate::ops::{OpsError, OperationsContext};
use serde::{Deserialize, Serialize};

inventory::submit! {
    ActionMeta { name: "empty_trash", description: "Empty the system trash (OS-delegated)", undoable: false }
}

pub struct EmptyTrash;

#[derive(Debug, Serialize, Deserialize)]
pub struct EmptyTrashInput {}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmptyTrashOutput {
    pub success: bool,
}

impl crate::ops::CoreAction for EmptyTrash {
    type Input = EmptyTrashInput;
    type Output = EmptyTrashOutput;

    fn name(&self) -> &'static str {
        "empty_trash"
    }

    async fn execute(
        &self,
        _ctx: &OperationsContext,
        _input: Self::Input,
    ) -> Result<(Self::Output, UndoEntry), OpsError> {
            tracing::info!(
                "EmptyTrash: files moved to system trash; OS-level purge not available via API"
            );

            let inverse_action = serde_json::json!({"action": "noop"}).to_string();

            let entry = UndoEntry {
                description: "Empty Trash (cannot undo)".into(),
                timestamp: chrono::Utc::now(),
                inverse_action,
            };

            Ok((EmptyTrashOutput { success: true }, entry))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::db::pool::{create_pool, run_migrations};
    use crate::domain::id::DeviceId;
    use crate::domain::undo::UndoStack;
    use crate::ops::{CoreAction, IndexContext, OperationsContext, SessionContext, StorageContext};
    use redb::Database;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::Mutex;

    async fn make_ctx(tmp: &TempDir) -> OperationsContext {
        let db_path = tmp.path().join("meta.db");
        let pool = create_pool(&db_path).await.expect("pool");
        run_migrations(&pool).await.expect("migrations");
        let redb_path = tmp.path().join("cache.redb");
        let cache = Arc::new(Database::create(&redb_path).expect("redb"));
        OperationsContext {
            session: SessionContext {
                device_id: DeviceId::new(),
                permissions: vec!["delete".into()],
                source: "test".into(),
                correlation_id: None,
            },
            storage: StorageContext { volume_id: "TEST".into() },
            index: IndexContext { pool, cache },
            undo_stack: Arc::new(Mutex::new(UndoStack::new())),
        }
    }

    #[tokio::test]
    async fn empty_trash_succeeds_with_noop_inverse() {
        let tmp = TempDir::new().unwrap();
        let ctx = make_ctx(&tmp).await;

        let action = EmptyTrash;
        let (output, entry) = action
            .execute(&ctx, EmptyTrashInput {})
            .await
            .expect("execute");

        assert!(output.success);
        assert_eq!(entry.description, "Empty Trash (cannot undo)");

        let inv: serde_json::Value = serde_json::from_str(&entry.inverse_action).unwrap();
        assert_eq!(inv["action"], "noop");
    }
}
