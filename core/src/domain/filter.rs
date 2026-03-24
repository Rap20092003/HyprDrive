//! FilterExpr — the query language of HyprDrive
//!
//! Powers all search, virtual folders, and disk insights.
//! A composable expression tree that compiles to SQL WHERE clauses.

use crate::domain::enums::FileCategory;
use crate::domain::id::TagId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// A SQL parameter value for prepared statements.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SqlParam {
    /// A text parameter.
    Text(String),
    /// An integer parameter.
    Int(i64),
    /// A float parameter.
    Float(f64),
}

/// A composable filter expression that compiles to a SQL WHERE clause.
///
/// Supports arbitrary nesting via `And`, `Or`, and `Not`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FilterExpr {
    /// All sub-expressions must match (logical AND).
    And(Vec<FilterExpr>),
    /// At least one sub-expression must match (logical OR).
    Or(Vec<FilterExpr>),
    /// The sub-expression must NOT match.
    Not(Box<FilterExpr>),
    /// Match files of a specific category (video, image, etc.)
    FileType(FileCategory),
    /// Match files with a specific extension.
    Extension(String),
    /// Match files within a size range (bytes, inclusive).
    SizeRange {
        /// Minimum size in bytes.
        min: u64,
        /// Maximum size in bytes.
        max: u64,
    },
    /// Match files within an allocated-on-disk range.
    AllocatedRange {
        /// Minimum allocated size in bytes.
        min: u64,
        /// Maximum allocated size in bytes.
        max: u64,
    },
    /// Match files modified within a date range.
    DateRange {
        /// Oldest modification date.
        start: DateTime<Utc>,
        /// Newest modification date.
        end: DateTime<Utc>,
    },
    /// Match files with a specific tag.
    Tag(TagId),
    /// Match files not accessed for at least this duration.
    StaleFor(Duration),
    /// Match files wasting disk space (allocated/size > threshold).
    IsWasteful(f64),
    /// Match build artifacts (node_modules, target, __pycache__, etc.)
    IsBuildArtifact,
    /// Match duplicate files (content hash has >1 location).
    Duplicate,
}

impl FilterExpr {
    /// Compile this expression into a SQL WHERE fragment and parameter list.
    ///
    /// Returns `(sql_fragment, params)` for use in prepared statements.
    pub fn compile_to_sql(&self) -> (String, Vec<SqlParam>) {
        let mut params = Vec::new();
        let sql = self.compile_inner(&mut params);
        (sql, params)
    }

    fn compile_inner(&self, params: &mut Vec<SqlParam>) -> String {
        match self {
            Self::And(exprs) => {
                if exprs.is_empty() {
                    return "1=1".to_string();
                }
                let parts: Vec<String> = exprs.iter().map(|e| e.compile_inner(params)).collect();
                format!("({})", parts.join(" AND "))
            }
            Self::Or(exprs) => {
                if exprs.is_empty() {
                    return "1=0".to_string();
                }
                let parts: Vec<String> = exprs.iter().map(|e| e.compile_inner(params)).collect();
                format!("({})", parts.join(" OR "))
            }
            Self::Not(expr) => {
                let inner = expr.compile_inner(params);
                format!("NOT ({})", inner)
            }
            Self::FileType(category) => {
                params.push(SqlParam::Text(format!("{:?}", category).to_lowercase()));
                "l.category = ?".to_string()
            }
            Self::Extension(ext) => {
                params.push(SqlParam::Text(ext.to_lowercase()));
                "l.extension = ?".to_string()
            }
            Self::SizeRange { min, max } => {
                // Clamp to i64::MAX to avoid wrapping u64::MAX → -1
                params.push(SqlParam::Int((*min).min(i64::MAX as u64) as i64));
                params.push(SqlParam::Int((*max).min(i64::MAX as u64) as i64));
                "l.size BETWEEN ? AND ?".to_string()
            }
            Self::AllocatedRange { min, max } => {
                params.push(SqlParam::Int((*min).min(i64::MAX as u64) as i64));
                params.push(SqlParam::Int((*max).min(i64::MAX as u64) as i64));
                "l.allocated_size BETWEEN ? AND ?".to_string()
            }
            Self::DateRange { start, end } => {
                params.push(SqlParam::Text(start.to_rfc3339()));
                params.push(SqlParam::Text(end.to_rfc3339()));
                "l.modified_at BETWEEN ? AND ?".to_string()
            }
            Self::Tag(tag_id) => {
                params.push(SqlParam::Text(tag_id.to_string()));
                "o.id IN (SELECT object_id FROM tags_on_objects WHERE tag_id = ?)".to_string()
            }
            Self::StaleFor(duration) => {
                let cutoff = Utc::now() - chrono::Duration::seconds(duration.as_secs() as i64);
                params.push(SqlParam::Text(cutoff.to_rfc3339()));
                "l.accessed_at < ?".to_string()
            }
            Self::IsWasteful(threshold) => {
                params.push(SqlParam::Float(*threshold));
                "CAST(l.allocated_size AS REAL) / MAX(l.size, 1) > ?".to_string()
            }
            Self::IsBuildArtifact => {
                "(l.materialized_path LIKE '%/node_modules/%' OR l.materialized_path LIKE '%/target/%' OR l.materialized_path LIKE '%/__pycache__/%' OR l.materialized_path LIKE '%/.git/objects/%' OR l.materialized_path LIKE '%/dist/%')".to_string()
            }
            Self::Duplicate => {
                "o.content_hash IN (SELECT content_hash FROM objects GROUP BY content_hash HAVING COUNT(*) > 1)".to_string()
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn and_composition() {
        let expr = FilterExpr::And(vec![
            FilterExpr::FileType(FileCategory::Image),
            FilterExpr::SizeRange {
                min: 1_000_000,
                max: u64::MAX,
            },
        ]);
        let (sql, params) = expr.compile_to_sql();
        assert_eq!(sql, "(l.category = ? AND l.size BETWEEN ? AND ?)");
        assert_eq!(params.len(), 3);
    }

    #[test]
    fn or_composition() {
        let expr = FilterExpr::Or(vec![
            FilterExpr::Extension("pdf".into()),
            FilterExpr::Extension("doc".into()),
        ]);
        let (sql, params) = expr.compile_to_sql();
        assert_eq!(sql, "(l.extension = ? OR l.extension = ?)");
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn not_composition() {
        let tag_id = TagId::new();
        let expr = FilterExpr::Not(Box::new(FilterExpr::Tag(tag_id)));
        let (sql, params) = expr.compile_to_sql();
        assert!(sql.starts_with("NOT ("));
        assert!(sql.contains("tag_id = ?"));
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn date_range() {
        let start = Utc::now() - chrono::Duration::days(30);
        let end = Utc::now();
        let expr = FilterExpr::DateRange { start, end };
        let (sql, params) = expr.compile_to_sql();
        assert_eq!(sql, "l.modified_at BETWEEN ? AND ?");
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn size_range() {
        let expr = FilterExpr::SizeRange {
            min: 1_073_741_824,  // 1 GB
            max: 10_737_418_240, // 10 GB
        };
        let (sql, params) = expr.compile_to_sql();
        assert_eq!(sql, "l.size BETWEEN ? AND ?");
        assert_eq!(params[0], SqlParam::Int(1_073_741_824));
        assert_eq!(params[1], SqlParam::Int(10_737_418_240));
    }

    #[test]
    fn allocated_range() {
        let expr = FilterExpr::AllocatedRange {
            min: 5_368_709_120,
            max: u64::MAX,
        };
        let (sql, params) = expr.compile_to_sql();
        assert_eq!(sql, "l.allocated_size BETWEEN ? AND ?");
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn stale_for() {
        let expr = FilterExpr::StaleFor(Duration::from_secs(730 * 24 * 3600));
        let (sql, params) = expr.compile_to_sql();
        assert_eq!(sql, "l.accessed_at < ?");
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn is_wasteful() {
        let expr = FilterExpr::IsWasteful(0.5);
        let (sql, params) = expr.compile_to_sql();
        assert!(sql.contains("CAST(l.allocated_size AS REAL)"));
        assert_eq!(params[0], SqlParam::Float(0.5));
    }

    #[test]
    fn is_build_artifact() {
        let expr = FilterExpr::IsBuildArtifact;
        let (sql, _params) = expr.compile_to_sql();
        assert!(sql.contains("node_modules"));
        assert!(sql.contains("target"));
        assert!(sql.contains("__pycache__"));
    }

    #[test]
    fn duplicate() {
        let expr = FilterExpr::Duplicate;
        let (sql, _params) = expr.compile_to_sql();
        assert!(sql.contains("HAVING COUNT(*) > 1"));
    }

    #[test]
    fn nested_composition() {
        let expr = FilterExpr::And(vec![
            FilterExpr::Or(vec![
                FilterExpr::Extension("pdf".into()),
                FilterExpr::Extension("doc".into()),
            ]),
            FilterExpr::Not(Box::new(FilterExpr::IsBuildArtifact)),
        ]);
        let (sql, params) = expr.compile_to_sql();
        assert!(sql.starts_with("("));
        assert!(sql.contains(" AND "));
        assert!(sql.contains(" OR "));
        assert!(sql.contains("NOT ("));
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn empty_and_or() {
        let (and_sql, _) = FilterExpr::And(vec![]).compile_to_sql();
        assert_eq!(and_sql, "1=1");

        let (or_sql, _) = FilterExpr::Or(vec![]).compile_to_sql();
        assert_eq!(or_sql, "1=0");
    }
}
