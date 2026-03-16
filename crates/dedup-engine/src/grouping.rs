//! Union-find grouping and reference selection for duplicate groups.
//!
//! Merges pairwise duplicate matches into transitive groups using union-find,
//! then selects a "reference" file (the likely original) for each group using
//! heuristics: shallowest path, oldest mtime, no "copy" pattern in name.

use crate::FileEntry;

/// The kind of match that linked two files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    /// Exact content match (same BLAKE3 hash).
    Content,
    /// Fuzzy filename match (Jaro-Winkler similarity).
    FuzzyFilename,
    /// Perceptual image match (blockhash Hamming distance).
    PerceptualImage,
}

/// A group of duplicate files with a selected reference.
#[derive(Debug, Clone)]
pub struct DupeGroup {
    /// The file deemed most likely to be the "original".
    pub reference: FileEntry,
    /// The duplicate files (excluding the reference).
    pub duplicates: Vec<FileEntry>,
    /// How the duplicates were detected.
    pub match_kind: MatchKind,
    /// Total bytes wasted by the duplicates (sum of duplicate sizes).
    pub total_wasted_bytes: u64,
}

/// Union-find (disjoint set) data structure with path compression and rank.
pub struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<usize>,
}

impl UnionFind {
    /// Create a new union-find with `n` elements, each in its own set.
    pub fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }

    /// Find the representative of the set containing `x` (with path compression).
    pub fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }

    /// Union the sets containing `x` and `y` (by rank).
    pub fn union(&mut self, x: usize, y: usize) {
        let rx = self.find(x);
        let ry = self.find(y);
        if rx == ry {
            return;
        }
        match self.rank[rx].cmp(&self.rank[ry]) {
            std::cmp::Ordering::Less => self.parent[rx] = ry,
            std::cmp::Ordering::Greater => self.parent[ry] = rx,
            std::cmp::Ordering::Equal => {
                self.parent[ry] = rx;
                self.rank[rx] += 1;
            }
        }
    }
}

/// Select the best reference from a group of files.
///
/// Heuristics (higher score = more likely original):
/// - Shallower path depth (fewer directory levels)
/// - Older modification time
/// - No "copy" pattern in filename
pub fn select_reference(files: &[FileEntry]) -> (FileEntry, Vec<FileEntry>) {
    assert!(
        !files.is_empty(),
        "cannot select reference from empty group"
    );

    if files.len() == 1 {
        return (files[0].clone(), Vec::new());
    }

    // Score each file: higher = more likely original
    let scored: Vec<(i64, usize)> = files
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let mut score: i64 = 0;

            // Prefer shallower paths (fewer components = more likely a "root" location)
            score -= (f.path_depth() as i64) * 10;

            // Penalize files with copy patterns in name
            if f.has_copy_pattern() {
                score -= 50;
            }

            // Prefer older files (lower mtime = older = more likely original)
            // Normalize: oldest file gets +5, others get 0
            score -= f.modified_at / 1_000_000; // coarse ranking by mtime

            (score, i)
        })
        .collect();

    let best_idx = scored
        .iter()
        .max_by_key(|(score, _)| *score)
        .map(|(_, idx)| *idx)
        .unwrap_or(0);

    let reference = files[best_idx].clone();
    let duplicates: Vec<FileEntry> = files
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != best_idx)
        .map(|(_, f)| f.clone())
        .collect();

    (reference, duplicates)
}

/// Group pairwise matches into transitive duplicate groups.
///
/// Uses union-find to merge: if A matches B and B matches C,
/// then {A, B, C} form one group. Selects a reference for each group.
#[tracing::instrument(skip_all, fields(file_count = files.len(), pair_count = pairs.len()))]
pub fn group_matches(files: &[FileEntry], pairs: Vec<(usize, usize, MatchKind)>) -> Vec<DupeGroup> {
    if pairs.is_empty() {
        return Vec::new();
    }

    let mut uf = UnionFind::new(files.len());

    // Track the match kind per pair (use the first kind seen per group)
    let mut kind_map = std::collections::HashMap::new();

    for &(a, b, kind) in &pairs {
        uf.union(a, b);
        kind_map.entry(uf.find(a)).or_insert(kind);
    }

    // Collect connected components
    let mut components: std::collections::HashMap<usize, Vec<usize>> =
        std::collections::HashMap::new();
    for &(a, _, _) in &pairs {
        let root = uf.find(a);
        components.entry(root).or_default();
    }
    // Re-iterate to collect all members
    for idx in 0..files.len() {
        let root = uf.find(idx);
        if let Some(members) = components.get_mut(&root) {
            members.push(idx);
        }
    }

    // Deduplicate indices within each component
    for members in components.values_mut() {
        members.sort_unstable();
        members.dedup();
    }

    // Build DupeGroups
    let mut groups = Vec::new();
    for (root, members) in &components {
        if members.len() < 2 {
            continue;
        }

        let group_files: Vec<FileEntry> = members.iter().map(|&i| files[i].clone()).collect();
        let match_kind = kind_map.get(root).copied().unwrap_or(MatchKind::Content);
        let (reference, duplicates) = select_reference(&group_files);
        let total_wasted_bytes: u64 = duplicates.iter().map(|f| f.size).sum();

        groups.push(DupeGroup {
            reference,
            duplicates,
            match_kind,
            total_wasted_bytes,
        });
    }

    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_entry(path: &str, size: u64, name: &str, mtime: i64) -> FileEntry {
        FileEntry {
            path: PathBuf::from(path),
            size,
            name: name.to_string(),
            extension: name.split('.').last().map(String::from),
            modified_at: mtime,
            inode: None,
        }
    }

    #[test]
    fn union_find_basic() {
        let mut uf = UnionFind::new(5);
        uf.union(0, 1);
        uf.union(1, 2);
        assert_eq!(uf.find(0), uf.find(2));
        assert_ne!(uf.find(0), uf.find(3));
    }

    #[test]
    fn union_find_transitivity() {
        let mut uf = UnionFind::new(4);
        uf.union(0, 1);
        uf.union(1, 2);
        uf.union(2, 3);
        let root = uf.find(0);
        assert_eq!(uf.find(1), root);
        assert_eq!(uf.find(2), root);
        assert_eq!(uf.find(3), root);
    }

    #[test]
    fn union_find_self_union() {
        let mut uf = UnionFind::new(3);
        uf.union(1, 1);
        assert_eq!(uf.find(1), 1);
    }

    #[test]
    fn select_reference_prefers_shallow() {
        let files = vec![
            make_entry("/photos/backup/deep/img.jpg", 100, "img.jpg", 1000),
            make_entry("/photos/img.jpg", 100, "img.jpg", 1000),
        ];
        let (reference, duplicates) = select_reference(&files);
        assert_eq!(reference.path, PathBuf::from("/photos/img.jpg"));
        assert_eq!(duplicates.len(), 1);
    }

    #[test]
    fn select_reference_penalizes_copy_pattern() {
        let files = vec![
            make_entry("/test/Copy of file.txt", 100, "Copy of file.txt", 1000),
            make_entry("/test/file.txt", 100, "file.txt", 1000),
        ];
        let (reference, duplicates) = select_reference(&files);
        assert_eq!(reference.path, PathBuf::from("/test/file.txt"));
        assert_eq!(duplicates.len(), 1);
    }

    #[test]
    fn select_reference_single_file() {
        let files = vec![make_entry("/test/file.txt", 100, "file.txt", 1000)];
        let (reference, duplicates) = select_reference(&files);
        assert_eq!(reference.name, "file.txt");
        assert!(duplicates.is_empty());
    }

    #[test]
    fn group_matches_transitive() {
        let files = vec![
            make_entry("/a.txt", 100, "a.txt", 0),
            make_entry("/b.txt", 100, "b.txt", 0),
            make_entry("/c.txt", 100, "c.txt", 0),
        ];
        // a=b, b=c → {a,b,c}
        let pairs = vec![(0, 1, MatchKind::Content), (1, 2, MatchKind::Content)];
        let groups = group_matches(&files, pairs);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].duplicates.len(), 2);
    }

    #[test]
    fn group_matches_two_separate_groups() {
        let files = vec![
            make_entry("/a.txt", 100, "a.txt", 0),
            make_entry("/b.txt", 100, "b.txt", 0),
            make_entry("/c.txt", 200, "c.txt", 0),
            make_entry("/d.txt", 200, "d.txt", 0),
        ];
        let pairs = vec![(0, 1, MatchKind::Content), (2, 3, MatchKind::Content)];
        let groups = group_matches(&files, pairs);
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn group_matches_empty_pairs() {
        let files = vec![make_entry("/a.txt", 100, "a.txt", 0)];
        let groups = group_matches(&files, Vec::new());
        assert!(groups.is_empty());
    }

    #[test]
    fn group_matches_wasted_bytes() {
        let files = vec![
            make_entry("/original.txt", 1000, "original.txt", 0),
            make_entry("/Copy of original.txt", 1000, "Copy of original.txt", 0),
            make_entry("/original (1).txt", 1000, "original (1).txt", 0),
        ];
        let pairs = vec![(0, 1, MatchKind::Content), (0, 2, MatchKind::Content)];
        let groups = group_matches(&files, pairs);
        assert_eq!(groups.len(), 1);
        // Reference = original.txt, duplicates = 2 × 1000 = 2000 wasted
        assert_eq!(groups[0].total_wasted_bytes, 2000);
    }
}
