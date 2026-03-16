//! Fuzzy filename matching for detecting renamed duplicates.
//!
//! Uses Jaro-Winkler similarity to find files that are likely copies
//! with slightly modified names (e.g. "report (1).pdf", "Copy of report.pdf").

use crate::FileEntry;
use std::collections::HashMap;

/// A fuzzy match between two files.
#[derive(Debug, Clone)]
pub struct FuzzyMatch {
    /// Index of first file in the input slice.
    pub idx_a: usize,
    /// Index of second file in the input slice.
    pub idx_b: usize,
    /// Similarity score (0.0–1.0).
    pub similarity: f64,
    /// Normalized name of file A (after stripping copy patterns).
    pub normalized_a: String,
    /// Normalized name of file B.
    pub normalized_b: String,
}

/// Normalize a filename for fuzzy comparison.
///
/// Strips common copy patterns:
/// - "Copy of X" → "x"
/// - "X (1)" → "x"
/// - "X - Copy" → "x"
/// - Lowercases everything
/// - Strips leading/trailing whitespace
pub fn normalize_name(name: &str) -> String {
    let s = name.to_lowercase();

    // Strip extension for comparison (we group by extension separately)
    let stem = if let Some(pos) = s.rfind('.') {
        &s[..pos]
    } else {
        &s
    }
    .to_string();

    let mut result = stem;

    // Strip "copy of " prefix
    if let Some(rest) = result.strip_prefix("copy of ") {
        result = rest.to_string();
    }

    // Strip " - copy" suffix
    if let Some(rest) = result.strip_suffix(" - copy") {
        result = rest.to_string();
    }

    // Strip " (N)" patterns like " (1)", " (2)", etc.
    let re_paren = regex_lite_strip_paren(&result);
    result = re_paren;

    // Strip trailing whitespace
    result.trim().to_string()
}

/// Strip " (N)" patterns from a string without regex dependency.
fn regex_lite_strip_paren(s: &str) -> String {
    let mut result = s.to_string();
    // Keep stripping trailing " (N)" patterns
    loop {
        let trimmed = result.trim_end();
        if let Some(open) = trimmed.rfind(" (") {
            let rest = &trimmed[open + 2..];
            if let Some(close_pos) = rest.find(')') {
                let inside = &rest[..close_pos];
                if inside.chars().all(|c| c.is_ascii_digit()) && close_pos == rest.len() - 1 {
                    result = trimmed[..open].to_string();
                    continue;
                }
            }
        }
        break;
    }
    result
}

/// Find files with similar names using Jaro-Winkler similarity.
///
/// Only compares files within the same extension group to reduce false positives.
/// Returns pairs where similarity exceeds `threshold` (default: 0.85).
#[tracing::instrument(skip(files), fields(file_count = files.len(), threshold))]
pub fn find_similar_names(files: &[FileEntry], threshold: f64) -> Vec<FuzzyMatch> {
    if files.is_empty() {
        return Vec::new();
    }

    // Group by extension
    let mut by_ext: HashMap<Option<&str>, Vec<(usize, &FileEntry)>> = HashMap::new();
    for (i, f) in files.iter().enumerate() {
        by_ext
            .entry(f.extension.as_deref())
            .or_default()
            .push((i, f));
    }

    let mut matches = Vec::new();

    for group in by_ext.values() {
        if group.len() < 2 {
            continue;
        }

        // Precompute normalized names
        let normalized: Vec<(usize, String)> = group
            .iter()
            .map(|(idx, f)| (*idx, normalize_name(&f.name)))
            .collect();

        // Pairwise comparison
        for i in 0..normalized.len() {
            for j in (i + 1)..normalized.len() {
                let sim = strsim::jaro_winkler(&normalized[i].1, &normalized[j].1);
                if sim >= threshold {
                    matches.push(FuzzyMatch {
                        idx_a: normalized[i].0,
                        idx_b: normalized[j].0,
                        similarity: sim,
                        normalized_a: normalized[i].1.clone(),
                        normalized_b: normalized[j].1.clone(),
                    });
                }
            }
        }
    }

    matches
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_entry(name: &str, ext: Option<&str>) -> FileEntry {
        FileEntry {
            path: PathBuf::from(format!("/test/{name}")),
            size: 100,
            name: name.to_string(),
            extension: ext.map(String::from),
            modified_at: 0,
            inode: None,
        }
    }

    #[test]
    fn normalize_copy_of() {
        assert_eq!(normalize_name("Copy of Budget.xlsx"), "budget");
    }

    #[test]
    fn normalize_numbered_copy() {
        assert_eq!(normalize_name("Report (1).pdf"), "report");
    }

    #[test]
    fn normalize_dash_copy() {
        assert_eq!(normalize_name("photo - Copy.jpg"), "photo");
    }

    #[test]
    fn normalize_unchanged() {
        assert_eq!(normalize_name("normal_file.txt"), "normal_file");
    }

    #[test]
    fn normalize_multiple_patterns() {
        assert_eq!(normalize_name("Copy of file (2).doc"), "file");
    }

    #[test]
    fn fuzzy_match_renamed_copy() {
        let files = vec![
            make_entry("report.pdf", Some("pdf")),
            make_entry("report (1).pdf", Some("pdf")),
        ];
        let matches = find_similar_names(&files, 0.85);
        assert_eq!(matches.len(), 1);
        assert!(matches[0].similarity > 0.85);
    }

    #[test]
    fn fuzzy_no_match_different_names() {
        let files = vec![
            make_entry("report.pdf", Some("pdf")),
            make_entry("invoice.pdf", Some("pdf")),
        ];
        let matches = find_similar_names(&files, 0.85);
        assert!(matches.is_empty());
    }

    #[test]
    fn fuzzy_no_match_different_extensions() {
        let files = vec![
            make_entry("photo.jpg", Some("jpg")),
            make_entry("photo.png", Some("png")),
        ];
        let matches = find_similar_names(&files, 0.85);
        // Different extensions = different groups = no comparison
        assert!(matches.is_empty());
    }

    #[test]
    fn fuzzy_empty_input() {
        let matches = find_similar_names(&[], 0.85);
        assert!(matches.is_empty());
    }
}
