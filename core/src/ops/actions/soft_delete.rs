//! `soft_delete` action — move files to the OS trash (Recycle Bin / macOS Trash).

use crate::db::queries::{delete_location_by_path, delete_orphan_objects};
use crate::domain::undo::UndoEntry;
use crate::ops::registry::ActionMeta;
use crate::ops::{OperationsContext, OpsError};
use serde::{Deserialize, Serialize};
use std::path::Path;

inventory::submit! {
    ActionMeta { name: "soft_delete", description: "Move files to the system trash", undoable: true }
}

pub struct SoftDelete;

#[derive(Debug, Serialize, Deserialize)]
pub struct SoftDeleteInput {
    pub paths: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SoftDeleteOutput {
    pub deleted_count: usize,
    pub orphaned_objects: usize,
}

impl crate::ops::CoreAction for SoftDelete {
    type Input = SoftDeleteInput;
    type Output = SoftDeleteOutput;

    fn name(&self) -> &'static str {
        "soft_delete"
    }

    async fn execute(
        &self,
        ctx: &OperationsContext,
        input: Self::Input,
    ) -> Result<(Self::Output, UndoEntry), OpsError> {
        let volume_id = &ctx.storage.volume_id;
        let mut deleted_count = 0usize;
        let mut orphaned_object_ids: Vec<String> = Vec::new();

        for path_str in &input.paths {
            let path = Path::new(path_str);

            if !path.exists() {
                return Err(OpsError::NotFound {
                    path: path_str.clone(),
                });
            }

            // Move to OS trash
            trash::delete(path).map_err(|e| OpsError::Trash(e.to_string()))?;

            // Remove location from DB; collect object_id for orphan cleanup
            if let Some(object_id) =
                delete_location_by_path(&ctx.index.pool, volume_id, path_str).await?
            {
                orphaned_object_ids.push(object_id);
            }

            deleted_count += 1;
        }

        // Clean up objects that no longer have any locations
        let orphaned_objects =
            delete_orphan_objects(&ctx.index.pool, &orphaned_object_ids).await? as usize;

        let inverse_action = serde_json::json!({
            "action": "restore_from_trash",
            "original_paths": input.paths,
        })
        .to_string();

        let entry = UndoEntry {
            description: format!("Deleted {} item(s) to trash", deleted_count),
            timestamp: chrono::Utc::now(),
            inverse_action,
        };

        Ok((
            SoftDeleteOutput {
                deleted_count,
                orphaned_objects,
            },
            entry,
        ))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::db::pool::{create_pool, run_migrations};
    use crate::db::queries::{lookup_location_by_path, upsert_location, upsert_object};
    use crate::db::types::{hash_state, LocationRow, ObjectRow};
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
            storage: StorageContext {
                volume_id: "TEST".into(),
            },
            index: IndexContext { pool, cache },
            undo_stack: Arc::new(Mutex::new(UndoStack::new())),
        }
    }

    #[tokio::test]
    async fn soft_delete_removes_from_fs_and_db() {
        let tmp = TempDir::new().unwrap();
        let ctx = make_ctx(&tmp).await;

        let file = tmp.path().join("to_delete.txt");
        tokio::fs::write(&file, b"bye").await.unwrap();
        let file_str = file.to_str().unwrap().to_string();

        // Register in DB
        let now = chrono::Utc::now().to_rfc3339();
        let obj_id = "deadbeef".repeat(4);
        upsert_object(
            &ctx.index.pool,
            &ObjectRow {
                id: obj_id.clone(),
                kind: "File".into(),
                mime_type: None,
                size_bytes: 3,
                created_at: now.clone(),
                updated_at: now.clone(),
                hash_state: hash_state::CONTENT.into(),
            },
        )
        .await
        .unwrap();
        upsert_location(
            &ctx.index.pool,
            &LocationRow {
                id: obj_id.clone(),
                object_id: obj_id.clone(),
                volume_id: "TEST".into(),
                path: file_str.clone(),
                name: "to_delete.txt".into(),
                extension: Some("txt".into()),
                parent_id: None,
                is_directory: false,
                size_bytes: 3,
                allocated_bytes: 3,
                created_at: now.clone(),
                modified_at: now.clone(),
                accessed_at: None,
                fid: None,
            },
        )
        .await
        .unwrap();

        let action = SoftDelete;
        let (output, entry) = action
            .execute(
                &ctx,
                SoftDeleteInput {
                    paths: vec![file_str.clone()],
                },
            )
            .await
            .expect("execute");

        // Verify file gone from FS (moved to trash)
        assert!(!file.exists());
        assert_eq!(output.deleted_count, 1);

        // Verify removed from DB
        let loc = lookup_location_by_path(&ctx.index.pool, "TEST", &file_str)
            .await
            .unwrap();
        assert!(loc.is_none());

        // Verify undo JSON
        let inv: serde_json::Value = serde_json::from_str(&entry.inverse_action).unwrap();
        assert_eq!(inv["action"], "restore_from_trash");
        assert_eq!(inv["original_paths"][0], file_str);
    }
}
