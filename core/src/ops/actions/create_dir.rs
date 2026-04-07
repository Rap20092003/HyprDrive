//! `create_dir` action — create a new directory on disk and register it in the DB.

use crate::db::queries::{upsert_location, upsert_object};
use crate::db::types::{hash_state, LocationRow, ObjectRow};
use crate::domain::undo::UndoEntry;
use crate::ops::{OpsError, OperationsContext};
use crate::ops::registry::ActionMeta;
use serde::{Deserialize, Serialize};
use std::path::Path;

inventory::submit! {
    ActionMeta { name: "create_dir", description: "Create a new directory", undoable: true }
}

pub struct CreateDir;

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateDirInput {
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateDirOutput {
    pub location_id: String,
}

impl crate::ops::CoreAction for CreateDir {
    type Input = CreateDirInput;
    type Output = CreateDirOutput;

    fn name(&self) -> &'static str {
        "create_dir"
    }

    async fn execute(
        &self,
        ctx: &OperationsContext,
        input: Self::Input,
    ) -> Result<(Self::Output, UndoEntry), OpsError> {
            let path = Path::new(&input.path);

            // Validate absolute path
            if !path.is_absolute() {
                return Err(OpsError::InvalidInput {
                    reason: "path must be absolute".into(),
                });
            }

            // Validate parent exists
            let parent = path.parent().ok_or_else(|| OpsError::InvalidInput {
                reason: "path has no parent".into(),
            })?;
            if !parent.exists() {
                return Err(OpsError::NotFound {
                    path: parent.to_string_lossy().into_owned(),
                });
            }

            // Validate dest doesn't already exist
            if path.exists() {
                return Err(OpsError::AlreadyExists {
                    path: input.path.clone(),
                });
            }

            // Create the directory
            tokio::fs::create_dir(path).await.map_err(OpsError::Io)?;

            let volume_id = &ctx.storage.volume_id;
            let path_str = input.path.as_str();

            // Compute location_id from volume+path
            let location_id = {
                let key = format!("{}:{}", volume_id, path_str);
                let hex = blake3::hash(key.as_bytes()).to_hex();
                hex[..32].to_string()
            };

            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();

            let now = chrono::Utc::now().to_rfc3339();

            let object_row = ObjectRow {
                id: location_id.clone(),
                kind: "Directory".into(),
                mime_type: None,
                size_bytes: 0,
                created_at: now.clone(),
                updated_at: now.clone(),
                hash_state: hash_state::CONTENT.into(),
            };

            let location_row = LocationRow {
                id: location_id.clone(),
                object_id: location_id.clone(),
                volume_id: volume_id.clone(),
                path: path_str.to_string(),
                name: name.clone(),
                extension: None,
                parent_id: None,
                is_directory: true,
                size_bytes: 0,
                allocated_bytes: 0,
                created_at: now.clone(),
                modified_at: now.clone(),
                accessed_at: None,
                fid: None,
            };

            upsert_object(&ctx.index.pool, &object_row).await?;
            upsert_location(&ctx.index.pool, &location_row).await?;

            let inverse_action =
                serde_json::json!({"action": "soft_delete", "paths": [path_str]}).to_string();

            let entry = UndoEntry {
                description: format!("Created directory {}", name),
                timestamp: chrono::Utc::now(),
                inverse_action,
            };

            Ok((CreateDirOutput { location_id }, entry))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::db::pool::{create_pool, run_migrations};
    use crate::domain::undo::UndoStack;
    use crate::ops::{CoreAction, IndexContext, OperationsContext, SessionContext, StorageContext};
    use crate::domain::id::DeviceId;
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
                permissions: vec!["write".into()],
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
    async fn create_dir_execute_and_undo_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let ctx = make_ctx(&tmp).await;
        let new_dir = tmp.path().join("my_new_dir");
        let path_str = new_dir.to_str().unwrap().to_string();

        let action = CreateDir;
        let (output, entry) = action
            .execute(&ctx, CreateDirInput { path: path_str.clone() })
            .await
            .expect("execute");

        // Verify directory created on disk
        assert!(new_dir.exists());
        assert!(new_dir.is_dir());

        // Verify location_id is non-empty 32-char hex
        assert_eq!(output.location_id.len(), 32);

        // Verify DB has the location
        let loc = crate::db::queries::lookup_location_by_path(
            &ctx.index.pool,
            "TEST",
            &path_str,
        )
        .await
        .expect("lookup")
        .expect("should exist");

        assert_eq!(loc.id, output.location_id);
        assert!(loc.is_directory);

        // Verify undo inverse JSON
        let inv: serde_json::Value =
            serde_json::from_str(&entry.inverse_action).expect("valid json");
        assert_eq!(inv["action"], "soft_delete");
        assert_eq!(inv["paths"][0], path_str);
    }
}
