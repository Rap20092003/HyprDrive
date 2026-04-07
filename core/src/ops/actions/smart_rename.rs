//! `smart_rename` action — bulk rename files using a date/metadata template.

use crate::db::queries::lookup_location_by_path;
use crate::domain::undo::UndoEntry;
use crate::ops::registry::ActionMeta;
use crate::ops::{OpsError, OperationsContext};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

inventory::submit! {
    ActionMeta { name: "smart_rename", description: "Bulk rename files using a date/metadata template", undoable: true }
}

pub struct SmartRename;

#[derive(Debug, Serialize, Deserialize)]
pub struct SmartRenameInput {
    pub source_paths: Vec<String>,
    /// Template string containing one or more of: `{year}`, `{month}`, `{day}`, `{original}`.
    pub template: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RenamedFile {
    pub original_path: String,
    pub new_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SkippedFile {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SmartRenameOutput {
    pub renamed: Vec<RenamedFile>,
    pub skipped: Vec<SkippedFile>,
}

/// Extracts (year, month, day) strings from EXIF DateTimeOriginal, falling back to mtime.
fn extract_date_from_exif_or_mtime(path: &Path) -> Result<(String, String, String), String> {
    // Try EXIF first
    if let Ok(file) = std::fs::File::open(path) {
        let mut bufreader = std::io::BufReader::new(file);
        if let Ok(exif_data) =
            exif::Reader::new().read_from_container(&mut bufreader)
        {
            if let Some(field) =
                exif_data.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)
            {
                // Format: "2024:01:15 14:30:00"
                let raw = field.display_value().to_string();
                let date_part = raw.split_whitespace().next().unwrap_or("");
                let parts: Vec<&str> = date_part.split(':').collect();
                if parts.len() == 3 {
                    let year = parts[0].trim_matches('"').to_string();
                    let month = parts[1].trim_matches('"').to_string();
                    let day = parts[2].trim_matches('"').to_string();
                    if year.len() == 4 && month.len() == 2 && day.len() == 2 {
                        return Ok((year, month, day));
                    }
                }
            }
        }
    }

    // Fallback: use file mtime
    let meta = std::fs::metadata(path)
        .map_err(|e| format!("metadata error: {}", e))?;
    let modified = meta
        .modified()
        .map_err(|e| format!("mtime error: {}", e))?;
    let secs = modified
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("time before epoch: {}", e))?
        .as_secs();

    // Convert Unix timestamp to year/month/day
    // Simple implementation using chrono
    let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(secs as i64, 0)
        .ok_or_else(|| "invalid timestamp".to_string())?;
    let year = format!("{:04}", dt.format("%Y"));
    let month = format!("{:02}", dt.format("%m"));
    let day = format!("{:02}", dt.format("%d"));
    Ok((year, month, day))
}

/// Apply the template substitutions to produce a new filename stem.
fn apply_template(template: &str, year: &str, month: &str, day: &str, original: &str) -> String {
    template
        .replace("{year}", year)
        .replace("{month}", month)
        .replace("{day}", day)
        .replace("{original}", original)
}

impl crate::ops::CoreAction for SmartRename {
    type Input = SmartRenameInput;
    type Output = SmartRenameOutput;

    fn name(&self) -> &'static str {
        "smart_rename"
    }

    async fn execute(
        &self,
        ctx: &OperationsContext,
        input: Self::Input,
    ) -> Result<(Self::Output, UndoEntry), OpsError> {
            // Validate template contains at least one recognised placeholder
            let has_placeholder = input.template.contains("{year}")
                || input.template.contains("{month}")
                || input.template.contains("{day}")
                || input.template.contains("{original}");

            if !has_placeholder {
                return Err(OpsError::InvalidInput {
                    reason: "template must contain at least one of: {year}, {month}, {day}, {original}".into(),
                });
            }

            let volume_id = &ctx.storage.volume_id;
            let mut renamed: Vec<RenamedFile> = Vec::new();
            let mut skipped: Vec<SkippedFile> = Vec::new();
            let mut inverse_moves: Vec<serde_json::Value> = Vec::new();

            for source_str in &input.source_paths {
                let source = PathBuf::from(source_str);

                // Extract date — errors go to skipped list
                let date_result = {
                    let source_clone = source.clone();
                    tokio::task::spawn_blocking(move || {
                        extract_date_from_exif_or_mtime(&source_clone)
                    })
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r)
                };

                let (year, month, day) = match date_result {
                    Ok(d) => d,
                    Err(e) => {
                        skipped.push(SkippedFile {
                            path: source_str.clone(),
                            reason: format!("date extraction failed: {}", e),
                        });
                        continue;
                    }
                };

                // Original stem (filename without extension)
                let original_stem = source
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();

                let original_extension: Option<String> = source
                    .extension()
                    .map(|e| e.to_string_lossy().into_owned());

                let new_stem = apply_template(
                    &input.template,
                    &year,
                    &month,
                    &day,
                    &original_stem,
                );

                let new_filename = match &original_extension {
                    Some(ext) => format!("{}.{}", new_stem, ext),
                    None => new_stem.clone(),
                };

                let dest_parent = source
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(PathBuf::new);

                let dest = dest_parent.join(&new_filename);
                let dest_str = dest.to_string_lossy().into_owned();

                // Create parent dirs if needed
                if let Some(parent) = dest.parent() {
                    if let Err(e) = tokio::fs::create_dir_all(parent).await {
                        skipped.push(SkippedFile {
                            path: source_str.clone(),
                            reason: format!("create_dir_all failed: {}", e),
                        });
                        continue;
                    }
                }

                // Perform rename
                if let Err(e) = tokio::fs::rename(&source, &dest).await {
                    skipped.push(SkippedFile {
                        path: source_str.clone(),
                        reason: format!("rename failed: {}", e),
                    });
                    continue;
                }

                // Update DB if location is tracked
                if let Ok(Some(loc)) =
                    lookup_location_by_path(&ctx.index.pool, volume_id, source_str).await
                {
                    let _ = sqlx::query(
                        "UPDATE locations SET path=?1, name=?2, modified_at=datetime('now') WHERE id=?3",
                    )
                    .bind(&dest_str)
                    .bind(&new_filename)
                    .bind(&loc.id)
                    .execute(&ctx.index.pool)
                    .await;
                }

                inverse_moves.push(serde_json::json!({
                    "from": dest_str,
                    "to": source_str,
                }));

                renamed.push(RenamedFile {
                    original_path: source_str.clone(),
                    new_path: dest_str,
                });
            }

            let inverse_action = serde_json::json!({
                "action": "bulk_move",
                "moves": inverse_moves,
            })
            .to_string();

            let entry = UndoEntry {
                description: format!(
                    "SmartRename: {} renamed, {} skipped",
                    renamed.len(),
                    skipped.len()
                ),
                timestamp: chrono::Utc::now(),
                inverse_action,
            };

            Ok((SmartRenameOutput { renamed, skipped }, entry))
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
    async fn smart_rename_template_renames_file_and_produces_inverse() {
        let tmp = TempDir::new().unwrap();
        let ctx = make_ctx(&tmp).await;

        let src = tmp.path().join("photo.jpg");
        tokio::fs::write(&src, b"fake image").await.unwrap();
        let src_str = src.to_str().unwrap().to_string();

        let action = SmartRename;
        let (output, entry) = action
            .execute(
                &ctx,
                SmartRenameInput {
                    source_paths: vec![src_str.clone()],
                    template: "{year}-{month}-{day}-{original}".into(),
                },
            )
            .await
            .expect("execute");

        // Should have renamed (not skipped) — mtime fallback is available
        assert_eq!(output.renamed.len(), 1, "expected 1 renamed, got: {:?}", output.skipped);
        assert!(output.skipped.is_empty());

        // Old path gone, new path exists
        assert!(!src.exists());
        let new_path = Path::new(&output.renamed[0].new_path);
        assert!(new_path.exists());

        // New filename matches template pattern YYYY-MM-DD-photo.jpg
        let new_name = new_path.file_name().unwrap().to_str().unwrap();
        assert!(
            new_name.len() > "photo.jpg".len(),
            "expected date-prefixed name, got: {}",
            new_name
        );

        // Undo has reverse mapping
        let inv: serde_json::Value = serde_json::from_str(&entry.inverse_action).unwrap();
        assert_eq!(inv["action"], "bulk_move");
        let moves = inv["moves"].as_array().unwrap();
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0]["to"], src_str);
    }
}
