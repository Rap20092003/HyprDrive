//! `bulk_tag` action — add or remove a tag from multiple objects atomically.

use crate::db::queries::{add_tags_batch, remove_tags_batch};
use crate::domain::undo::UndoEntry;
use crate::ops::registry::ActionMeta;
use crate::ops::{OpsError, OperationsContext};
use serde::{Deserialize, Serialize};

inventory::submit! {
    ActionMeta { name: "bulk_tag", description: "Add or remove a tag from multiple objects", undoable: true }
}

pub struct BulkTag;

#[derive(Debug, Serialize, Deserialize)]
pub struct BulkTagInput {
    pub object_ids: Vec<String>,
    pub tag_id: String,
    /// Either `"add"` or `"remove"`.
    pub operation: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BulkTagOutput {
    pub affected_count: u64,
}

impl crate::ops::CoreAction for BulkTag {
    type Input = BulkTagInput;
    type Output = BulkTagOutput;

    fn name(&self) -> &'static str {
        "bulk_tag"
    }

    async fn execute(
        &self,
        ctx: &OperationsContext,
        input: Self::Input,
    ) -> Result<(Self::Output, UndoEntry), OpsError> {
            // Validate operation
            if input.operation != "add" && input.operation != "remove" {
                return Err(OpsError::InvalidInput {
                    reason: format!(
                        "operation must be 'add' or 'remove', got '{}'",
                        input.operation
                    ),
                });
            }

            // Validate tag_id non-empty
            if input.tag_id.is_empty() {
                return Err(OpsError::InvalidInput {
                    reason: "tag_id must not be empty".into(),
                });
            }

            // Validate object_ids non-empty
            if input.object_ids.is_empty() {
                return Err(OpsError::InvalidInput {
                    reason: "object_ids must not be empty".into(),
                });
            }

            let affected_count = if input.operation == "add" {
                add_tags_batch(&ctx.index.pool, &input.tag_id, &input.object_ids).await?
            } else {
                remove_tags_batch(&ctx.index.pool, &input.tag_id, &input.object_ids).await?
            };

            // Inverse flips the operation
            let reverse_op = if input.operation == "add" { "remove" } else { "add" };
            let inverse_action = serde_json::json!({
                "action": "bulk_tag",
                "object_ids": input.object_ids,
                "tag_id": input.tag_id,
                "operation": reverse_op,
            })
            .to_string();

            let entry = UndoEntry {
                description: format!(
                    "{}d tag '{}' on {} object(s)",
                    if input.operation == "add" { "Add" } else { "Remove" },
                    input.tag_id,
                    input.object_ids.len(),
                ),
                timestamp: chrono::Utc::now(),
                inverse_action,
            };

            Ok((BulkTagOutput { affected_count }, entry))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::db::pool::{create_pool, run_migrations};
    use crate::db::queries::{upsert_object, tags_for_object};
    use crate::db::types::{hash_state, ObjectRow};
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

    async fn insert_objects_and_tag(ctx: &OperationsContext, object_ids: &[&str], tag_id: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        for oid in object_ids {
            upsert_object(
                &ctx.index.pool,
                &ObjectRow {
                    id: oid.to_string(),
                    kind: "File".into(),
                    mime_type: None,
                    size_bytes: 0,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                    hash_state: hash_state::CONTENT.into(),
                },
            )
            .await
            .unwrap();
        }
        // Insert tag row
        sqlx::query("INSERT OR IGNORE INTO tags (id, name) VALUES (?1, ?2)")
            .bind(tag_id)
            .bind("test-tag")
            .execute(&ctx.index.pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn bulk_tag_add_then_remove_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let ctx = make_ctx(&tmp).await;

        let object_ids = vec![
            "obj001aa".repeat(4).to_string(),
            "obj002bb".repeat(4).to_string(),
        ];
        let tag_id = "tag001cc".repeat(4).to_string();

        insert_objects_and_tag(&ctx, &[&object_ids[0], &object_ids[1]], &tag_id).await;

        let action = BulkTag;

        // Add
        let (add_output, add_entry) = action
            .execute(
                &ctx,
                BulkTagInput {
                    object_ids: object_ids.clone(),
                    tag_id: tag_id.clone(),
                    operation: "add".into(),
                },
            )
            .await
            .expect("add execute");

        assert_eq!(add_output.affected_count, 2);
        let tags = tags_for_object(&ctx.index.pool, &object_ids[0]).await.unwrap();
        assert_eq!(tags.len(), 1);

        // Verify inverse JSON
        let inv: serde_json::Value = serde_json::from_str(&add_entry.inverse_action).unwrap();
        assert_eq!(inv["operation"], "remove");

        // Remove (execute inverse)
        let (remove_output, _) = action
            .execute(
                &ctx,
                BulkTagInput {
                    object_ids: object_ids.clone(),
                    tag_id: tag_id.clone(),
                    operation: "remove".into(),
                },
            )
            .await
            .expect("remove execute");

        assert_eq!(remove_output.affected_count, 2);
        let tags_after = tags_for_object(&ctx.index.pool, &object_ids[0]).await.unwrap();
        assert!(tags_after.is_empty());
    }
}
