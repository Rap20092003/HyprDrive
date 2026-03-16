//! Duplicate scanner orchestrator.
//!
//! Coordinates the multi-strategy scanning pipeline:
//! 1. Size bucketing → eliminate unique-size files
//! 2. Partial BLAKE3 hash → eliminate unique-partial files
//! 3. Full BLAKE3 hash → confirm exact content duplicates
//! 4. Optional: fuzzy filename matching, perceptual image matching
//! 5. Union-find grouping → transitive duplicate groups

use crate::error::DeduplicateResult;
use crate::grouping::{group_matches, DupeGroup, MatchKind};
use crate::FileEntry;
use rayon::prelude::*;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Scanning strategy to use.
#[derive(Debug, Clone)]
pub enum ScanStrategy {
    /// Exact content matching via progressive BLAKE3 hashing.
    Content,
    /// Fuzzy filename matching with Jaro-Winkler similarity.
    FuzzyFilename {
        /// Minimum similarity threshold (0.0–1.0). Default: 0.85.
        threshold: f64,
    },
    /// Perceptual image matching with blockhash.
    PerceptualImage {
        /// Maximum Hamming distance for a match. Default: 10.
        threshold: u32,
    },
}

/// A content match group: files sharing the same BLAKE3 hash.
#[derive(Debug, Clone)]
pub struct ContentMatch {
    /// The full BLAKE3 hash shared by all files in this group.
    pub hash: [u8; 32],
    /// Indices of files with this hash (into the original input slice).
    pub file_indices: Vec<usize>,
}

/// Complete duplicate scan report.
#[derive(Debug, Clone)]
pub struct DupeReport {
    /// Duplicate groups found.
    pub groups: Vec<DupeGroup>,
    /// Total bytes wasted by duplicates.
    pub total_duplicate_bytes: u64,
    /// Total number of duplicate files (excluding references).
    pub total_duplicate_files: usize,
    /// How long the scan took.
    pub scan_duration: Duration,
    /// Number of files scanned.
    pub files_scanned: usize,
    /// Number of files skipped due to errors.
    pub files_skipped: usize,
    /// Names of strategies used.
    pub strategies_used: Vec<String>,
}

impl std::fmt::Display for DupeReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== Duplicate Scan Report ===")?;
        writeln!(f, "Files scanned:    {}", self.files_scanned)?;
        writeln!(f, "Files skipped:    {}", self.files_skipped)?;
        writeln!(f, "Duplicate groups: {}", self.groups.len())?;
        writeln!(f, "Duplicate files:  {}", self.total_duplicate_files)?;
        writeln!(
            f,
            "Wasted space:     {} bytes ({:.2} MB)",
            self.total_duplicate_bytes,
            self.total_duplicate_bytes as f64 / (1024.0 * 1024.0)
        )?;
        writeln!(f, "Scan duration:    {:.2?}", self.scan_duration)?;
        writeln!(
            f,
            "Strategies:       {}",
            self.strategies_used.join(", ")
        )?;
        Ok(())
    }
}

/// Configurable duplicate file scanner.
pub struct DuplicateScanner {
    /// Strategies to apply.
    strategies: Vec<ScanStrategy>,
    /// Minimum file size to consider (skip smaller files).
    min_size: u64,
    /// Maximum file size to consider (skip larger files).
    max_size: Option<u64>,
}

impl Default for DuplicateScanner {
    fn default() -> Self {
        Self {
            strategies: vec![ScanStrategy::Content],
            min_size: 1, // skip empty files by default
            max_size: None,
        }
    }
}

impl DuplicateScanner {
    /// Create a new scanner with default Content strategy.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a scanning strategy.
    #[must_use]
    pub fn with_strategy(mut self, strategy: ScanStrategy) -> Self {
        self.strategies.push(strategy);
        self
    }

    /// Set the minimum file size to consider.
    #[must_use]
    pub fn with_min_size(mut self, min_size: u64) -> Self {
        self.min_size = min_size;
        self
    }

    /// Set the maximum file size to consider.
    #[must_use]
    pub fn with_max_size(mut self, max_size: u64) -> Self {
        self.max_size = Some(max_size);
        self
    }

    /// Clear default strategies (to build a custom set).
    #[must_use]
    pub fn clear_strategies(mut self) -> Self {
        self.strategies.clear();
        self
    }

    /// Run the full scan pipeline on the given files.
    #[tracing::instrument(skip(self, files), fields(
        file_count = files.len(),
        strategy_count = self.strategies.len(),
        min_size = self.min_size,
    ))]
    pub fn scan(&self, files: &[FileEntry]) -> DeduplicateResult<DupeReport> {
        let start = Instant::now();

        // Filter files by size constraints
        let filtered: Vec<&FileEntry> = files
            .iter()
            .filter(|f| {
                f.size >= self.min_size
                    && self.max_size.map_or(true, |max| f.size <= max)
            })
            .collect();

        let files_scanned = filtered.len();
        let files_skipped = files.len() - files_scanned;
        let mut all_pairs: Vec<(usize, usize, MatchKind)> = Vec::new();
        let mut strategies_used = Vec::new();

        for strategy in &self.strategies {
            match strategy {
                ScanStrategy::Content => {
                    strategies_used.push("Content (BLAKE3)".to_string());
                    let content_pairs = scan_content(files, self.min_size);
                    all_pairs.extend(content_pairs);
                }
                ScanStrategy::FuzzyFilename { threshold } => {
                    strategies_used.push(format!("Fuzzy Filename (threshold={threshold})"));
                    let fuzzy_matches = crate::fuzzy::find_similar_names(files, *threshold);
                    for m in fuzzy_matches {
                        all_pairs.push((m.idx_a, m.idx_b, MatchKind::FuzzyFilename));
                    }
                }
                ScanStrategy::PerceptualImage { threshold } => {
                    strategies_used.push(format!("Perceptual Image (threshold={threshold})"));
                    #[cfg(feature = "perceptual")]
                    {
                        match crate::perceptual::find_similar_images(files, *threshold) {
                            Ok(perceptual_matches) => {
                                for m in perceptual_matches {
                                    all_pairs.push((
                                        m.idx_a,
                                        m.idx_b,
                                        MatchKind::PerceptualImage,
                                    ));
                                }
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "Perceptual scanning failed, skipping");
                            }
                        }
                    }
                    #[cfg(not(feature = "perceptual"))]
                    {
                        tracing::warn!("Perceptual scanning requested but feature not enabled");
                    }
                }
            }
        }

        let groups = group_matches(files, all_pairs);
        let total_duplicate_bytes: u64 = groups.iter().map(|g| g.total_wasted_bytes).sum();
        let total_duplicate_files: usize = groups.iter().map(|g| g.duplicates.len()).sum();

        Ok(DupeReport {
            groups,
            total_duplicate_bytes,
            total_duplicate_files,
            scan_duration: start.elapsed(),
            files_scanned,
            files_skipped,
            strategies_used,
        })
    }
}

/// Group files by size, returning only buckets with 2+ files.
///
/// Files with unique sizes cannot be content duplicates.
#[tracing::instrument(skip_all, fields(file_count = files.len(), min_size))]
pub fn group_by_size<'a>(
    files: &'a [FileEntry],
    min_size: u64,
) -> HashMap<u64, Vec<(usize, &'a FileEntry)>> {
    let mut buckets: HashMap<u64, Vec<(usize, &FileEntry)>> = HashMap::new();

    for (i, f) in files.iter().enumerate() {
        if f.size < min_size {
            continue;
        }
        buckets.entry(f.size).or_default().push((i, f));
    }

    // Remove unique-size buckets
    buckets.retain(|_, v| v.len() >= 2);
    buckets
}

/// Content-based duplicate scanning pipeline.
///
/// 1. Size bucket
/// 2. Partial hash within each bucket (parallel)
/// 3. Full hash within each partial-match group (parallel)
/// 4. Collect pairs of matching files
fn scan_content(
    all_files: &[FileEntry],
    min_size: u64,
) -> Vec<(usize, usize, MatchKind)> {
    let size_buckets = group_by_size(all_files, min_size);
    let mut result_pairs: Vec<(usize, usize, MatchKind)> = Vec::new();

    for bucket in size_buckets.values() {
        if bucket.len() < 2 {
            continue;
        }

        // Step 2: Partial hash within bucket
        let partial_results: Vec<Option<(usize, [u8; 32])>> = bucket
            .par_iter()
            .map(|(idx, f)| match crate::hasher::partial_hash(&f.path) {
                Ok(hash) => Some((*idx, hash)),
                Err(e) => {
                    tracing::warn!(path = %f.path.display(), error = %e, "Partial hash failed, skipping");
                    None
                }
            })
            .collect();

        let valid_partials: Vec<(usize, [u8; 32])> =
            partial_results.into_iter().flatten().collect();

        // Group by partial hash
        let mut partial_groups: HashMap<[u8; 32], Vec<usize>> = HashMap::new();
        for (idx, hash) in &valid_partials {
            partial_groups.entry(*hash).or_default().push(*idx);
        }

        // Step 3: Full hash within each partial-match group
        for group in partial_groups.values() {
            if group.len() < 2 {
                continue;
            }

            let full_results: Vec<Option<(usize, [u8; 32])>> = group
                .par_iter()
                .map(|&idx| {
                    match crate::hasher::full_hash(&all_files[idx].path) {
                        Ok(hash) => Some((idx, hash)),
                        Err(e) => {
                            tracing::warn!(
                                path = %all_files[idx].path.display(),
                                error = %e,
                                "Full hash failed, skipping"
                            );
                            None
                        }
                    }
                })
                .collect();

            let valid_fulls: Vec<(usize, [u8; 32])> =
                full_results.into_iter().flatten().collect();

            // Group by full hash
            let mut full_groups: HashMap<[u8; 32], Vec<usize>> = HashMap::new();
            for (idx, hash) in &valid_fulls {
                full_groups.entry(*hash).or_default().push(*idx);
            }

            // Step 4: Emit pairs for groups with 2+ matches
            for indices in full_groups.values() {
                if indices.len() < 2 {
                    continue;
                }
                // Create pairs between all members
                for i in 0..indices.len() {
                    for j in (i + 1)..indices.len() {
                        result_pairs.push((
                            indices[i],
                            indices[j],
                            MatchKind::Content,
                        ));
                    }
                }
            }
        }
    }

    result_pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_entry(name: &str, size: u64) -> FileEntry {
        FileEntry {
            path: PathBuf::from(format!("/test/{name}")),
            size,
            name: name.to_string(),
            extension: name.split('.').last().map(String::from),
            modified_at: 0,
            inode: None,
        }
    }

    #[test]
    fn group_by_size_basic() {
        let files = vec![
            make_entry("a.txt", 100),
            make_entry("b.txt", 100),
            make_entry("c.txt", 200),
            make_entry("d.txt", 200),
            make_entry("e.txt", 300),
        ];
        let buckets = group_by_size(&files, 1);
        assert_eq!(buckets.len(), 2); // 100 and 200
        assert_eq!(buckets[&100].len(), 2);
        assert_eq!(buckets[&200].len(), 2);
        assert!(!buckets.contains_key(&300)); // unique size
    }

    #[test]
    fn group_by_size_all_unique() {
        let files = vec![
            make_entry("a.txt", 100),
            make_entry("b.txt", 200),
            make_entry("c.txt", 300),
        ];
        let buckets = group_by_size(&files, 1);
        assert!(buckets.is_empty());
    }

    #[test]
    fn group_by_size_min_size_filter() {
        let files = vec![
            make_entry("tiny.txt", 50),
            make_entry("small.txt", 50),
            make_entry("big.txt", 1000),
            make_entry("big2.txt", 1000),
        ];
        let buckets = group_by_size(&files, 100);
        assert_eq!(buckets.len(), 1); // only 1000
        assert!(!buckets.contains_key(&50));
    }

    #[test]
    fn group_by_size_empty() {
        let files: Vec<FileEntry> = Vec::new();
        let buckets = group_by_size(&files, 1);
        assert!(buckets.is_empty());
    }

    #[test]
    fn scanner_content_finds_duplicates() {
        let dir = tempfile::TempDir::new().unwrap();

        // Create 3 identical files
        let content = b"duplicate content for testing";
        let _paths: Vec<_> = (0..3)
            .map(|i| {
                let p = dir.path().join(format!("dup_{i}.txt"));
                std::fs::write(&p, content).unwrap();
                p
            })
            .collect();

        // Create 2 unique files
        for i in 0..2 {
            let p = dir.path().join(format!("unique_{i}.txt"));
            std::fs::write(&p, format!("unique content {i}")).unwrap();
        }

        let files: Vec<FileEntry> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter_map(|e| FileEntry::from_path(e.path()).ok())
            .collect();

        let scanner = DuplicateScanner::new();
        let report = scanner.scan(&files).unwrap();

        // Should find 1 content group with 3 files
        let content_groups: Vec<_> = report
            .groups
            .iter()
            .filter(|g| g.match_kind == MatchKind::Content)
            .collect();
        assert_eq!(content_groups.len(), 1);
        assert_eq!(content_groups[0].duplicates.len(), 2); // 1 ref + 2 dupes
    }

    #[test]
    fn scanner_all_unique() {
        let dir = tempfile::TempDir::new().unwrap();

        for i in 0..5 {
            let p = dir.path().join(format!("file_{i}.txt"));
            std::fs::write(&p, format!("unique content {i}")).unwrap();
        }

        let files: Vec<FileEntry> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter_map(|e| FileEntry::from_path(e.path()).ok())
            .collect();

        let scanner = DuplicateScanner::new();
        let report = scanner.scan(&files).unwrap();
        assert!(report.groups.is_empty());
    }

    #[test]
    fn scanner_same_size_different_content() {
        let dir = tempfile::TempDir::new().unwrap();

        // Two files, same size, different content
        let p1 = dir.path().join("file1.txt");
        let p2 = dir.path().join("file2.txt");
        std::fs::write(&p1, "content_A!").unwrap(); // 10 bytes
        std::fs::write(&p2, "content_B!").unwrap(); // 10 bytes

        let files = vec![
            FileEntry::from_path(&p1).unwrap(),
            FileEntry::from_path(&p2).unwrap(),
        ];

        let scanner = DuplicateScanner::new();
        let report = scanner.scan(&files).unwrap();
        assert!(report.groups.is_empty());
    }

    #[test]
    fn scanner_empty_input() {
        let scanner = DuplicateScanner::new();
        let report = scanner.scan(&[]).unwrap();
        assert!(report.groups.is_empty());
        assert_eq!(report.total_duplicate_bytes, 0);
        assert_eq!(report.total_duplicate_files, 0);
    }

    #[test]
    fn scanner_no_strategies() {
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path().join("file.txt");
        std::fs::write(&p, "hello").unwrap();
        let files = vec![FileEntry::from_path(&p).unwrap()];

        let scanner = DuplicateScanner::new().clear_strategies();
        let report = scanner.scan(&files).unwrap();
        assert!(report.groups.is_empty());
    }

    #[test]
    fn dupe_report_display() {
        let report = DupeReport {
            groups: Vec::new(),
            total_duplicate_bytes: 0,
            total_duplicate_files: 0,
            scan_duration: Duration::from_millis(42),
            files_scanned: 100,
            files_skipped: 5,
            strategies_used: vec!["Content (BLAKE3)".to_string()],
        };
        let s = report.to_string();
        assert!(s.contains("Files scanned:    100"));
        assert!(s.contains("Content (BLAKE3)"));
    }
}
