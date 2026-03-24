//! Tag domain type — named, colored, hierarchical labels.

use crate::domain::id::TagId;
use serde::{Deserialize, Serialize};

/// A named label that can be applied to any object.
///
/// Tags are hierarchical (parent/child) and carry three name forms
/// per the Architecture spec:
/// - `canonical_name`: filesystem-safe slug ("work-projects-active")
/// - `display_name`: human-readable label ("Active Projects")
/// - `formal_name`: full hierarchical path ("Work/Projects/Active")
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tag {
    /// Unique identifier.
    pub id: TagId,
    /// Filesystem-safe, lowercase, hyphenated slug.
    ///
    /// Example: `"work-projects-active"`
    pub canonical_name: String,
    /// Human-readable display label.
    ///
    /// Example: `"Active Projects"`
    pub display_name: String,
    /// Full hierarchical path for display and querying.
    ///
    /// Example: `"Work/Projects/Active"`
    pub formal_name: String,
    /// Hex color for UI display, e.g. `"#FF5733"`.
    pub color: Option<String>,
    /// Parent tag ID for hierarchical nesting. `None` = root tag.
    pub parent_id: Option<TagId>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn make_tag() -> Tag {
        Tag {
            id: TagId::new(),
            canonical_name: "work-projects-active".into(),
            display_name: "Active Projects".into(),
            formal_name: "Work/Projects/Active".into(),
            color: Some("#FF5733".into()),
            parent_id: None,
        }
    }

    #[test]
    fn tag_serde_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let tag = make_tag();
        let json = serde_json::to_string(&tag)?;
        let back: Tag = serde_json::from_str(&json)?;
        assert_eq!(tag.canonical_name, back.canonical_name);
        assert_eq!(tag.display_name, back.display_name);
        assert_eq!(tag.formal_name, back.formal_name);
        assert_eq!(tag.color, back.color);
        Ok(())
    }

    #[test]
    fn tag_child_has_parent_id() {
        let parent = make_tag();
        let child = Tag {
            id: TagId::new(),
            canonical_name: "work-projects-active-rust".into(),
            display_name: "Rust".into(),
            formal_name: "Work/Projects/Active/Rust".into(),
            color: None,
            parent_id: Some(parent.id),
        };
        assert_eq!(child.parent_id, Some(parent.id));
        assert!(child.color.is_none());
    }

    #[test]
    fn tag_root_has_no_parent() {
        let tag = make_tag();
        assert!(tag.parent_id.is_none());
    }

    #[test]
    fn tag_formal_name_mirrors_hierarchy() {
        let tag = make_tag();
        assert!(tag.formal_name.contains('/'));
        assert!(tag.formal_name.ends_with("Active"));
    }
}
