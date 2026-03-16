//! End-to-end integration tests for the dedup engine.

use hyprdrive_dedup_engine::{
    DuplicateScanner, FileEntry, ScanStrategy,
    grouping::MatchKind,
};
use std::io::Write;
use tempfile::TempDir;

/// Helper: create a file with specific content in a temp directory.
fn create_file(dir: &std::path::Path, name: &str, content: &[u8]) -> std::path::PathBuf {
    let path = dir.join(name);
    let mut f = std::fs::File::create(&path).expect("create file");
    f.write_all(content).expect("write file");
    f.flush().expect("flush file");
    path
}

/// Collect FileEntry for all files in a directory.
fn collect_entries(dir: &std::path::Path) -> Vec<FileEntry> {
    std::fs::read_dir(dir)
        .expect("read dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .filter_map(|e| FileEntry::from_path(e.path()).ok())
        .collect()
}

#[test]
fn content_duplicates_five_copies() {
    let dir = TempDir::new().expect("tempdir");
    let content = b"hello world duplicate content for testing purposes";

    // 5 copies of the same content
    for i in 0..5 {
        create_file(dir.path(), &format!("copy_{i}.txt"), content);
    }
    // 3 unique files
    for i in 0..3 {
        create_file(
            dir.path(),
            &format!("unique_{i}.txt"),
            format!("unique content number {i}").as_bytes(),
        );
    }

    let files = collect_entries(dir.path());
    assert_eq!(files.len(), 8);

    let scanner = DuplicateScanner::new();
    let report = scanner.scan(&files).expect("scan");

    // Should find exactly 1 content group
    let content_groups: Vec<_> = report
        .groups
        .iter()
        .filter(|g| g.match_kind == MatchKind::Content)
        .collect();
    assert_eq!(content_groups.len(), 1);
    // 1 reference + 4 duplicates
    assert_eq!(content_groups[0].duplicates.len(), 4);
    // Wasted bytes = 4 * content.len()
    assert_eq!(
        content_groups[0].total_wasted_bytes,
        4 * content.len() as u64
    );
}

#[test]
fn fuzzy_filename_matching() {
    let dir = TempDir::new().expect("tempdir");

    // Create files with similar names but different content
    create_file(dir.path(), "report.pdf", b"original report content");
    create_file(dir.path(), "report (1).pdf", b"different content entirely");

    let files = collect_entries(dir.path());
    assert_eq!(files.len(), 2);

    let scanner = DuplicateScanner::new()
        .clear_strategies()
        .with_strategy(ScanStrategy::FuzzyFilename { threshold: 0.85 });
    let report = scanner.scan(&files).expect("scan");

    // Should find 1 fuzzy match group
    let fuzzy_groups: Vec<_> = report
        .groups
        .iter()
        .filter(|g| g.match_kind == MatchKind::FuzzyFilename)
        .collect();
    assert_eq!(fuzzy_groups.len(), 1);
}

#[test]
fn zero_byte_files_handled() {
    let dir = TempDir::new().expect("tempdir");

    // Create empty files
    create_file(dir.path(), "empty1.txt", b"");
    create_file(dir.path(), "empty2.txt", b"");
    create_file(dir.path(), "empty3.txt", b"");

    let files = collect_entries(dir.path());
    assert_eq!(files.len(), 3);

    // Default scanner skips empty files (min_size=1)
    let scanner = DuplicateScanner::new();
    let report = scanner.scan(&files).expect("scan");
    assert!(report.groups.is_empty());

    // With min_size=0, empty files should be grouped
    let scanner_zero = DuplicateScanner::new().with_min_size(0);
    let report_zero = scanner_zero.scan(&files).expect("scan");
    // All empty files have the same hash → 1 group
    let content_groups: Vec<_> = report_zero
        .groups
        .iter()
        .filter(|g| g.match_kind == MatchKind::Content)
        .collect();
    assert_eq!(content_groups.len(), 1);
    assert_eq!(content_groups[0].duplicates.len(), 2); // 1 ref + 2 dupes
}

#[test]
fn scanner_with_no_strategies_empty_report() {
    let dir = TempDir::new().expect("tempdir");
    create_file(dir.path(), "file.txt", b"hello");

    let files = collect_entries(dir.path());
    let scanner = DuplicateScanner::new().clear_strategies();
    let report = scanner.scan(&files).expect("scan");
    assert!(report.groups.is_empty());
    assert_eq!(report.total_duplicate_bytes, 0);
}

#[test]
fn ten_groups_of_ten_duplicates() {
    let dir = TempDir::new().expect("tempdir");

    // Create 10 groups of 10 duplicate files each
    for group in 0..10 {
        let content = format!("group_{group}_content_padding_to_make_unique");
        for copy in 0..10 {
            create_file(
                dir.path(),
                &format!("group{group}_copy{copy}.txt"),
                content.as_bytes(),
            );
        }
    }

    let files = collect_entries(dir.path());
    assert_eq!(files.len(), 100);

    let scanner = DuplicateScanner::new();
    let report = scanner.scan(&files).expect("scan");

    // Should find exactly 10 groups
    assert_eq!(report.groups.len(), 10);
    // Each group has 1 reference + 9 duplicates
    for group in &report.groups {
        assert_eq!(group.duplicates.len(), 9);
    }
    assert_eq!(report.total_duplicate_files, 90);
}

#[test]
fn mixed_strategies_content_and_fuzzy() {
    let dir = TempDir::new().expect("tempdir");

    // Content duplicates
    create_file(dir.path(), "data_a.bin", b"identical binary content");
    create_file(dir.path(), "data_b.bin", b"identical binary content");

    // Fuzzy name matches (different content)
    create_file(dir.path(), "report.pdf", b"original report");
    create_file(dir.path(), "report (1).pdf", b"modified report copy");

    let files = collect_entries(dir.path());

    let scanner = DuplicateScanner::new()
        .with_strategy(ScanStrategy::FuzzyFilename { threshold: 0.85 });
    let report = scanner.scan(&files).expect("scan");

    // Should have at least 1 content group and 1 fuzzy group
    let content_count = report
        .groups
        .iter()
        .filter(|g| g.match_kind == MatchKind::Content)
        .count();
    let fuzzy_count = report
        .groups
        .iter()
        .filter(|g| g.match_kind == MatchKind::FuzzyFilename)
        .count();

    assert!(content_count >= 1, "expected content duplicates");
    assert!(fuzzy_count >= 1, "expected fuzzy matches");
}

#[test]
fn report_display_format() {
    let dir = TempDir::new().expect("tempdir");
    create_file(dir.path(), "a.txt", b"dup");
    create_file(dir.path(), "b.txt", b"dup");

    let files = collect_entries(dir.path());
    let scanner = DuplicateScanner::new();
    let report = scanner.scan(&files).expect("scan");

    let display = report.to_string();
    assert!(display.contains("Duplicate Scan Report"));
    assert!(display.contains("Files scanned:"));
    assert!(display.contains("Duplicate groups:"));
}
