//! `move_file` action — move a file to a new location (same or cross-volume).

use crate::db::queries::lookup_location_by_path;
use crate::domain::undo::UndoEntry;
use crate::ops::registry::ActionMeta;
use crate::ops::{OperationsContext, OpsError};
use serde::{Deserialize, Serialize};
use std::path::Path;

inventory::submit! {
    ActionMeta { name: "move_file", description: "Move a file to a new location", undoable: true }
}

pub struct MoveFile;

#[derive(Debug, Serialize, Deserialize)]
pub struct MoveFileInput {
    pub source_path: String,
    pub dest_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MoveFileOutput {
    pub new_path: String,
}

impl crate::ops::CoreAction for MoveFile {
    type Input = MoveFileInput;
    type Output = MoveFileOutput;

    fn name(&self) -> &'static str {
        "move_file"
    }

    async fn execute(
        &self,
        ctx: &OperationsContext,
        input: Self::Input,
    ) -> Result<(Self::Output, UndoEntry), OpsError> {
        let source = Path::new(&input.source_path);
        let dest = Path::new(&input.dest_path);

        // Validate source exists
        if !source.exists() {
            return Err(OpsError::NotFound {
                path: input.source_path.clone(),
            });
        }

        // Validate dest parent exists
        let dest_parent = dest.parent().ok_or_else(|| OpsError::InvalidInput {
            reason: "dest_path has no parent".into(),
        })?;
        if !dest_parent.exists() {
            return Err(OpsError::NotFound {
                path: dest_parent.to_string_lossy().into_owned(),
            });
        }

        // Validate dest does not already exist
        if dest.exists() {
            return Err(OpsError::AlreadyExists {
                path: input.dest_path.clone(),
            });
        }

        let dest_str = input.dest_path.as_str();

        // Try fast same-volume rename first; fall back to copy+delete for cross-device
        match tokio::fs::rename(source, dest).await {
            Ok(()) => {}
            Err(e) if e.raw_os_error() == Some(18) || e.raw_os_error() == Some(17) => {
                // EXDEV (18 on Linux) or cross-device — copy then delete
                tokio::fs::copy(source, dest).await.map_err(OpsError::Io)?;
                tokio::fs::remove_file(source).await.map_err(OpsError::Io)?;
            }
            Err(e) => return Err(OpsError::Io(e)),
        }

        let volume_id = &ctx.storage.volume_id;
        let dest_name = dest
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dest_extension: Option<String> =
            dest.extension().map(|e| e.to_string_lossy().into_owned());

        // Update DB location if source was tracked
        if let Some(loc) =
            lookup_location_by_path(&ctx.index.pool, volume_id, &input.source_path).await?
        {
            sqlx::query(
                    "UPDATE locations SET path=?1, name=?2, extension=?3, modified_at=datetime('now') WHERE id=?4",
                )
                .bind(dest_str)
                .bind(&dest_name)
                .bind(&dest_extension)
                .bind(&loc.id)
                .execute(&ctx.index.pool)
                .await?;
        }

        let inverse_action = serde_json::json!({
            "action": "move_file",
            "source_path": dest_str,
            "dest_path": input.source_path,
        })
        .to_string();

        let entry = UndoEntry {
            description: format!("Moved {} to {}", input.source_path, dest_str),
            timestamp: chrono::Utc::now(),
            inverse_action,
        };

        Ok((
            MoveFileOutput {
                new_path: dest_str.to_string(),
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
            storage: StorageContext {
                volume_id: "TEST".into(),
            },
            index: IndexContext { pool, cache },
            undo_stack: Arc::new(Mutex::new(UndoStack::new())),
        }
    }

    #[tokio::test]
    async fn move_file_updates_disk_and_db() {
        let tmp = TempDir::new().unwrap();
        let ctx = make_ctx(&tmp).await;

        let src = tmp.path().join("original.txt");
        tokio::fs::write(&src, b"move me").await.unwrap();
        let src_str = src.to_str().unwrap().to_string();
        let dest = tmp.path().join("moved.txt");
        let dest_str = dest.to_str().unwrap().to_string();

        // Register source in DB
        let now = chrono::Utc::now().to_rfc3339();
        let obj_id = "ccddaabb".repeat(4);
        upsert_object(
            &ctx.index.pool,
            &ObjectRow {
                id: obj_id.clone(),
                kind: "File".into(),
                mime_type: None,
                size_bytes: 7,
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
                path: src_str.clone(),
                name: "original.txt".into(),
                extension: Some("txt".into()),
                parent_id: None,
                is_directory: false,
                size_bytes: 7,
                allocated_bytes: 7,
                created_at: now.clone(),
                modified_at: now.clone(),
                accessed_at: None,
                fid: None,
            },
        )
        .await
        .unwrap();

        let action = MoveFile;
        let (output, entry) = action
            .execute(
                &ctx,
                MoveFileInput {
                    source_path: src_str.clone(),
                    dest_path: dest_str.clone(),
                },
            )
            .await
            .expect("execute");

        // Verify disk state
        assert!(!src.exists());
        assert!(dest.exists());
        assert_eq!(output.new_path, dest_str);

        // Verify DB updated
        let loc = lookup_location_by_path(&ctx.index.pool, "TEST", &dest_str)
            .await
            .unwrap()
            .expect("location at dest");
        assert_eq!(loc.name, "moved.txt");

        // Verify undo JSON
        let inv: serde_json::Value = serde_json::from_str(&entry.inverse_action).unwrap();
        assert_eq!(inv["action"], "move_file");
        assert_eq!(inv["source_path"], dest_str);
        assert_eq!(inv["dest_path"], src_str);
    }
}
