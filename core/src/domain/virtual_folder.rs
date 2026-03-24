//! Virtual folders — saved filter queries that act as dynamic folders.

use crate::domain::filter::FilterExpr;
use crate::domain::id::VirtualFolderId;
use serde::{Deserialize, Serialize};

/// A virtual folder is a saved query that acts like a dynamic folder.
/// Its contents are determined by a `FilterExpr` evaluated at display time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VirtualFolder {
    /// Unique identifier for this virtual folder.
    pub id: VirtualFolderId,
    /// Human-readable name.
    pub name: String,
    /// Filter expression defining folder contents.
    pub filter: FilterExpr,
    /// Whether this folder is pinned to the sidebar.
    pub pinned: bool,
    /// Optional icon identifier (emoji or icon name).
    pub icon: Option<String>,
    /// Optional display color (hex string).
    pub color: Option<String>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::domain::enums::FileCategory;

    #[test]
    fn virtual_folder_serde_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let folder = VirtualFolder {
            id: VirtualFolderId::new(),
            name: "Big Videos".into(),
            filter: FilterExpr::And(vec![
                FilterExpr::FileType(FileCategory::Video),
                FilterExpr::SizeRange {
                    min: 1_000_000_000,
                    max: u64::MAX,
                },
            ]),
            pinned: true,
            icon: Some("🎬".into()),
            color: Some("#ff6600".into()),
        };
        let json = serde_json::to_string(&folder)?;
        let back: VirtualFolder = serde_json::from_str(&json)?;
        assert_eq!(folder.name, back.name);
        assert_eq!(folder.pinned, back.pinned);
        assert_eq!(folder.icon, back.icon);
        Ok(())
    }

    #[test]
    fn virtual_folder_with_empty_filter() {
        let folder = VirtualFolder {
            id: VirtualFolderId::new(),
            name: "Everything".into(),
            filter: FilterExpr::And(vec![]),
            pinned: false,
            icon: None,
            color: None,
        };
        let (sql, _) = folder.filter.compile_to_sql();
        assert_eq!(sql, "1=1"); // empty AND = match everything
    }
}
