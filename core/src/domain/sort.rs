//! Sort fields — how file listings can be ordered.

use serde::{Deserialize, Serialize};

/// A field that files can be sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortField {
    /// Sort by file/folder name.
    Name,
    /// Sort by file size in bytes.
    Size,
    /// Sort by last modified timestamp.
    Modified,
    /// Sort by creation timestamp.
    Created,
    /// Sort by file extension.
    Extension,
    /// Sort by file category.
    Category,
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortDirection {
    /// Ascending (A→Z, smallest→largest, oldest→newest).
    Asc,
    /// Descending (Z→A, largest→smallest, newest→oldest).
    Desc,
}

impl SortField {
    /// The SQL column name this field maps to.
    pub fn sql_column(self) -> &'static str {
        match self {
            Self::Name => "l.name",
            Self::Size => "l.size",
            Self::Modified => "l.modified_at",
            Self::Created => "l.created_at",
            Self::Extension => "l.extension",
            Self::Category => "l.category",
        }
    }

    /// Build a full SQL ORDER BY fragment like `"l.name ASC"`.
    pub fn to_sql_fragment(self, dir: SortDirection) -> String {
        let dir_str = match dir {
            SortDirection::Asc => "ASC",
            SortDirection::Desc => "DESC",
        };
        format!("{} {}", self.sql_column(), dir_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_field_sql_columns() {
        assert_eq!(SortField::Name.sql_column(), "l.name");
        assert_eq!(SortField::Size.sql_column(), "l.size");
        assert_eq!(SortField::Modified.sql_column(), "l.modified_at");
        assert_eq!(SortField::Created.sql_column(), "l.created_at");
        assert_eq!(SortField::Extension.sql_column(), "l.extension");
        assert_eq!(SortField::Category.sql_column(), "l.category");
    }

    #[test]
    fn sort_field_to_sql_fragment() {
        assert_eq!(
            SortField::Name.to_sql_fragment(SortDirection::Asc),
            "l.name ASC"
        );
        assert_eq!(
            SortField::Size.to_sql_fragment(SortDirection::Desc),
            "l.size DESC"
        );
    }

    #[test]
    fn sort_field_serde_roundtrip() {
        let field = SortField::Modified;
        let json = serde_json::to_string(&field).ok().unwrap();
        let back: SortField = serde_json::from_str(&json).ok().unwrap();
        assert_eq!(field, back);
    }
}
