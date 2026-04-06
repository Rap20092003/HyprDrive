//! Build artifact and cache directory detection patterns.
//!
//! Used to identify directories that can be safely deleted to reclaim space
//! (e.g., node_modules, target, __pycache__).

/// Known build artifact / cache directory names.
pub const BUILD_ARTIFACT_PATTERNS: &[&str] = &[
    "node_modules",
    "target",
    "__pycache__",
    ".git/objects",
    "dist",
    ".next",
    ".nuxt",
    "build",
    ".gradle",
    ".cache",
    "vendor",
    ".tox",
    ".pytest_cache",
    ".mypy_cache",
    "egg-info",
];

/// Check if a directory name matches a known build artifact pattern.
///
/// Case-insensitive comparison on the directory name.
pub fn is_build_artifact_dir(name: &str) -> bool {
    let lower = name.to_lowercase();
    BUILD_ARTIFACT_PATTERNS.iter().any(|p| lower == *p)
}

/// Generate a SQL fragment for WHERE clause filtering.
///
/// Returns something like: `LOWER(l.name) IN ('node_modules', 'target', ...)`
/// Caller is responsible for placing it in a valid SQL context.
pub fn build_artifact_sql_fragment() -> String {
    let quoted: Vec<String> = BUILD_ARTIFACT_PATTERNS
        .iter()
        .map(|p| format!("'{p}'"))
        .collect();
    format!("LOWER(l.name) IN ({})", quoted.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_patterns_match() {
        assert!(is_build_artifact_dir("node_modules"));
        assert!(is_build_artifact_dir("target"));
        assert!(is_build_artifact_dir("__pycache__"));
        assert!(is_build_artifact_dir(".gradle"));
        assert!(is_build_artifact_dir("NODE_MODULES")); // case-insensitive
    }

    #[test]
    fn non_patterns_dont_match() {
        assert!(!is_build_artifact_dir("src"));
        assert!(!is_build_artifact_dir("Documents"));
        assert!(!is_build_artifact_dir("my_target_folder"));
        assert!(!is_build_artifact_dir(""));
    }

    #[test]
    fn sql_fragment_covers_all_patterns() {
        let sql = build_artifact_sql_fragment();
        assert!(sql.starts_with("LOWER(l.name) IN ("));
        for p in BUILD_ARTIFACT_PATTERNS {
            assert!(sql.contains(&format!("'{p}'")), "missing pattern: {p}");
        }
    }
}
