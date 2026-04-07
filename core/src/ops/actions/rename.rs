//! `rename` action — rename a file or directory in place.

use crate::db::queries::lookup_location_by_path;
use crate::domain::undo::UndoEntry;
use crate::ops::registry::ActionMeta;
use crate::ops::{OpsError, OperationsContext};
use serde::{Deserialize, Serialize};
use std::path::Path;

inventory::submit! {
    ActionMeta { name: "rename", description: "Rename a file or directory", undoable: true }
}

pub struct Rename;

#[derive(Debug, Serialize, Deserialize)]
pub struct RenameInput {
    pub path: String,
    pub new_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RenameOutput {
    pub new_path: String,
}

impl crate::ops::CoreAction for Rename {
    type Input = RenameInput;
    type Output = RenameOutput;

    fn name(&self) -> &'static str {
        "rename"
    }

    async fn execute(
        &self,
        ctx: &OperationsContext,
        input: Self::Input,
    ) -> Result<(Self::Output, UndoEntry), OpsError> {
            let path = Path::new(&input.path);

            // Validate source exists
            if !path.exists() {
                return Err(OpsError::NotFound {
                    path: input.path.clone(),
                });
            }

            // Validate new_name is non-empty
            if input.new_name.is_empty() {
                return Err(OpsError::InvalidInput {
                    reason: "new_name must not be empty".into(),
                });
            }

            // Validate new_name has no path separators
            if input.new_name.contains('/') || input.new_name.contains('\\') {
                return Err(OpsError::InvalidInput {
                    reason: "new_name must not contain path separators".into(),
                });
            }

            let parent = path.parent().ok_or_else(|| OpsError::InvalidInput {
                reason: "path has no parent".into(),
            })?;

            let new_path = parent.join(&input.new_name);
            let new_path_str = new_path.to_string_lossy().into_owned();

            // Validate dest doesn't already exist
            if new_path.exists() {
                return Err(OpsError::AlreadyExists {
                    path: new_path_str.clone(),
                });
            }

            // Preserve old name for inverse
            let old_name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();

            // Perform the rename
            tokio::fs::rename(path, &new_path)
                .await
                .map_err(OpsError::Io)?;

            // Update DB if location exists
            let volume_id = &ctx.storage.volume_id;
            if let Some(loc) =
                lookup_location_by_path(&ctx.index.pool, volume_id, &input.path).await?
            {
                // Extract extension from new name
                let new_extension: Option<String> = Path::new(&input.new_name)
                    .extension()
                    .map(|e| e.to_string_lossy().into_owned());

                sqlx::query(
                    "UPDATE locations SET path=?1, name=?2, extension=?3, modified_at=datetime('now') WHERE id=?4",
                )
                .bind(&new_path_str)
                .bind(&input.new_name)
                .bind(&new_extension)
                .bind(&loc.id)
                .execute(&ctx.index.pool)
                .await?;
            }

            let inverse_action = serde_json::json!({
                "action": "rename",
                "path": new_path_str,
                "new_name": old_name,
            })
            .to_string();

            let entry = UndoEntry {
                description: format!("Renamed {} to {}", old_name, input.new_name),
                timestamp: chrono::Utc::now(),
                inverse_action,
            };

            Ok((RenameOutput { new_path: new_path_str }, entry))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::db::pool::{create_pool, run_migrations};
    use crate::db::queries::{upsert_location, upsert_object};
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
                permissions: vec!["write".into()],
                source: "test".into(),
                correlation_id: None,
            },
            storage: StorageContext { volume_id: "TEST".into() },
            index: IndexContext { pool, cache },
            undo_stack: Arc::new(Mutex::new(UndoStack::new())),
        }
    }

    #[tokio::test]
    async fn rename_file_updates_disk_and_db() {
        let tmp = TempDir::new().unwrap();
        let ctx = make_ctx(&tmp).await;

        let src = tmp.path().join("original.txt");
        tokio::fs::write(&src, b"hello").await.unwrap();
        let src_str = src.to_str().unwrap().to_string();

        // Insert location into DB
        let now = chrono::Utc::now().to_rfc3339();
        let obj_id = "aabbccdd".repeat(4);
        let obj = ObjectRow {
            id: obj_id.clone(),
            kind: "File".into(),
            mime_type: None,
            size_bytes: 5,
            created_at: now.clone(),
            updated_at: now.clone(),
            hash_state: hash_state::CONTENT.into(),
        };
        upsert_object(&ctx.index.pool, &obj).await.unwrap();
        let loc = LocationRow {
            id: obj_id.clone(),
            object_id: obj_id.clone(),
            volume_id: "TEST".into(),
            path: src_str.clone(),
            name: "original.txt".into(),
            extension: Some("txt".into()),
            parent_id: None,
            is_directory: false,
            size_bytes: 5,
            allocated_bytes: 5,
            created_at: now.clone(),
            modified_at: now.clone(),
            accessed_at: None,
            fid: None,
        };
        upsert_location(&ctx.index.pool, &loc).await.unwrap();

        let action = Rename;
        let (output, entry) = action
            .execute(
                &ctx,
                RenameInput { path: src_str.clone(), new_name: "renamed.txt".into() },
            )
            .await
            .expect("execute");

        // Old path gone, new path exists
        assert!(!src.exists());
        let new_path = tmp.path().join("renamed.txt");
        assert!(new_path.exists());
        assert_eq!(output.new_path, new_path.to_str().unwrap());

        // DB updated
        let updated =
            lookup_location_by_path(&ctx.index.pool, "TEST", &output.new_path)
                .await
                .unwrap()
                .expect("location at new path");
        assert_eq!(updated.name, "renamed.txt");
        assert_eq!(updated.extension, Some("txt".into()));

        // Undo JSON
        let inv: serde_json::Value = serde_json::from_str(&entry.inverse_action).unwrap();
        assert_eq!(inv["action"], "rename");
        assert_eq!(inv["new_name"], "original.txt");
    }
}
