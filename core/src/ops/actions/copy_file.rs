//! `copy_file` action — copy a file to a new destination.

use crate::db::queries::{upsert_location, upsert_object};
use crate::db::types::{hash_state, LocationRow, ObjectRow};
use crate::domain::undo::UndoEntry;
use crate::ops::registry::ActionMeta;
use crate::ops::{OperationsContext, OpsError};
use serde::{Deserialize, Serialize};
use std::path::Path;

inventory::submit! {
    ActionMeta { name: "copy_file", description: "Copy a file to a new destination", undoable: true }
}

pub struct CopyFile;

#[derive(Debug, Serialize, Deserialize)]
pub struct CopyFileInput {
    pub source_path: String,
    pub dest_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CopyFileOutput {
    pub dest_location_id: String,
    pub bytes_copied: u64,
}

impl crate::ops::CoreAction for CopyFile {
    type Input = CopyFileInput;
    type Output = CopyFileOutput;

    fn name(&self) -> &'static str {
        "copy_file"
    }

    async fn execute(
        &self,
        ctx: &OperationsContext,
        input: Self::Input,
    ) -> Result<(Self::Output, UndoEntry), OpsError> {
        let source = Path::new(&input.source_path);
        let dest = Path::new(&input.dest_path);

        // Validate source exists and is a file
        if !source.exists() {
            return Err(OpsError::NotFound {
                path: input.source_path.clone(),
            });
        }
        if !source.is_file() {
            return Err(OpsError::InvalidInput {
                reason: "source_path must be a file".into(),
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

        // Copy the file
        let bytes_copied = tokio::fs::copy(source, dest).await.map_err(OpsError::Io)?;

        // Hash the destination file in a blocking task
        let dest_owned = dest.to_path_buf();
        let hash_hex = tokio::task::spawn_blocking(move || {
            let mut hasher = blake3::Hasher::new();
            let mut file = std::fs::File::open(&dest_owned)?;
            std::io::copy(&mut file, &mut hasher)?;
            Ok::<String, std::io::Error>(hasher.finalize().to_hex().to_string())
        })
        .await
        .map_err(|e| OpsError::TaskPanicked(e.to_string()))??;

        let volume_id = &ctx.storage.volume_id;
        let dest_str = input.dest_path.as_str();

        // Dest location_id from volume+path
        let dest_location_id = {
            let key = format!("{}:{}", volume_id, dest_str);
            let hex = blake3::hash(key.as_bytes()).to_hex();
            hex[..32].to_string()
        };

        let dest_name = dest
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let dest_extension: Option<String> =
            dest.extension().map(|e| e.to_string_lossy().into_owned());

        // Use full 64-char hash as object id (content-addressed)
        let object_id = hash_hex.clone();

        let now = chrono::Utc::now().to_rfc3339();

        let object_row = ObjectRow {
            id: object_id.clone(),
            kind: "File".into(),
            mime_type: None,
            size_bytes: bytes_copied as i64,
            created_at: now.clone(),
            updated_at: now.clone(),
            hash_state: hash_state::CONTENT.into(),
        };

        let location_row = LocationRow {
            id: dest_location_id.clone(),
            object_id: object_id.clone(),
            volume_id: volume_id.clone(),
            path: dest_str.to_string(),
            name: dest_name,
            extension: dest_extension,
            parent_id: None,
            is_directory: false,
            size_bytes: bytes_copied as i64,
            allocated_bytes: bytes_copied as i64,
            created_at: now.clone(),
            modified_at: now.clone(),
            accessed_at: None,
            fid: None,
        };

        upsert_object(&ctx.index.pool, &object_row).await?;
        upsert_location(&ctx.index.pool, &location_row).await?;

        let inverse_action =
            serde_json::json!({"action": "soft_delete", "paths": [dest_str]}).to_string();

        let entry = UndoEntry {
            description: format!("Copied {} to {}", input.source_path, dest_str),
            timestamp: chrono::Utc::now(),
            inverse_action,
        };

        Ok((
            CopyFileOutput {
                dest_location_id,
                bytes_copied,
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
    use crate::db::queries::lookup_location_by_path;
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
    async fn copy_file_creates_dest_and_registers_db() {
        let tmp = TempDir::new().unwrap();
        let ctx = make_ctx(&tmp).await;

        let src = tmp.path().join("source.txt");
        tokio::fs::write(&src, b"copy content").await.unwrap();
        let dest = tmp.path().join("dest.txt");

        let action = CopyFile;
        let (output, entry) = action
            .execute(
                &ctx,
                CopyFileInput {
                    source_path: src.to_str().unwrap().to_string(),
                    dest_path: dest.to_str().unwrap().to_string(),
                },
            )
            .await
            .expect("execute");

        // Source still exists, dest created
        assert!(src.exists());
        assert!(dest.exists());
        assert_eq!(output.bytes_copied, 12);

        // Same content
        let content = tokio::fs::read(&dest).await.unwrap();
        assert_eq!(content, b"copy content");

        // DB has dest location
        let loc = lookup_location_by_path(&ctx.index.pool, "TEST", dest.to_str().unwrap())
            .await
            .unwrap()
            .expect("location should exist");
        assert_eq!(loc.id, output.dest_location_id);
        assert!(!loc.is_directory);

        // Undo JSON
        let inv: serde_json::Value = serde_json::from_str(&entry.inverse_action).unwrap();
        assert_eq!(inv["action"], "soft_delete");
        assert_eq!(inv["paths"][0], dest.to_str().unwrap());
    }
}
