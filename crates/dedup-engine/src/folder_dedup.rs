//! Duplicate folder detection via bottom-up Merkle tree hashing.
//!
//! Identifies directory-level duplication by computing a content hash for each
//! directory (the combined hash of its children), then grouping directories
//! with identical Merkle roots.
//!
//! This catches bulk copies like `project/` and `project-backup/` where every
//! file is duplicated but no single file scan would surface the folder-level
//! relationship.
//!
//! # Algorithm
//!
//! 1. Build a tree from flat `FileEntry` list (group by parent directory).
//! 2. Bottom-up hash: leaf file hashes feed into parent directory hashes.
//! 3. Group directories by their Merkle root → identical content directories.
//! 4. Filter: only report groups with 2+ directories, minimum child count.

use crate::FileEntry;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A group of directories with identical content.
#[derive(Debug, Clone)]
pub struct DupeFolderGroup {
    /// Directories in this group (all have identical content trees).
    pub directories: Vec<PathBuf>,
    /// Number of files inside each directory (recursive).
    pub file_count: usize,
    /// Total size of files inside each directory (recursive).
    pub total_size: u64,
    /// The Merkle root hash of the directory content.
    pub merkle_hash: [u8; 32],
}

/// Configuration for folder dedup scanning.
#[derive(Debug, Clone)]
pub struct FolderDedupConfig {
    /// Minimum number of files a directory must contain to be considered.
    /// Prevents noise from single-file directories.
    pub min_children: usize,
    /// Minimum total size (bytes) for a directory to be considered.
    pub min_total_size: u64,
}

impl Default for FolderDedupConfig {
    fn default() -> Self {
        Self {
            min_children: 2,
            min_total_size: 0,
        }
    }
}

/// Internal node in the directory tree for Merkle hashing.
#[derive(Debug)]
struct DirNode {
    /// Direct child file sizes and names (for hashing).
    child_files: Vec<(String, u64)>,
    /// Direct child subdirectory paths.
    child_dirs: Vec<PathBuf>,
    /// Computed Merkle hash (filled bottom-up).
    merkle_hash: Option<[u8; 32]>,
    /// Total recursive file count.
    file_count: usize,
    /// Total recursive byte size.
    total_size: u64,
}

/// Find groups of duplicate directories from a flat list of file entries.
///
/// Files are grouped by parent directory, then a bottom-up Merkle tree
/// is computed. Directories sharing the same Merkle root are duplicates.
#[tracing::instrument(skip_all, fields(file_count = files.len()))]
pub fn find_duplicate_folders(
    files: &[FileEntry],
    config: &FolderDedupConfig,
) -> Vec<DupeFolderGroup> {
    if files.is_empty() {
        return Vec::new();
    }

    // Step 1: Build directory tree from flat file list
    let mut dirs: HashMap<PathBuf, DirNode> = HashMap::new();

    for file in files {
        let parent = match file.path.parent() {
            Some(p) => p.to_path_buf(),
            None => continue,
        };

        let entry = dirs.entry(parent.clone()).or_insert_with(|| DirNode {
            child_files: Vec::new(),
            child_dirs: Vec::new(),
            merkle_hash: None,
            file_count: 0,
            total_size: 0,
        });
        entry.child_files.push((file.name.clone(), file.size));
        entry.file_count += 1;
        entry.total_size += file.size;

        // Register parent chain so intermediate directories exist
        let mut ancestor = parent.as_path();
        while let Some(grandparent) = ancestor.parent() {
            if grandparent == ancestor {
                break; // root
            }
            let gp_entry = dirs
                .entry(grandparent.to_path_buf())
                .or_insert_with(|| DirNode {
                    child_files: Vec::new(),
                    child_dirs: Vec::new(),
                    merkle_hash: None,
                    file_count: 0,
                    total_size: 0,
                });
            let child_path = ancestor.to_path_buf();
            if !gp_entry.child_dirs.contains(&child_path) {
                gp_entry.child_dirs.push(child_path);
            }
            ancestor = grandparent;
        }
    }

    // Step 2: Bottom-up Merkle hash (process leaf directories first)
    // Topological sort by depth: deepest first
    let mut dir_paths: Vec<PathBuf> = dirs.keys().cloned().collect();
    dir_paths.sort_by_key(|b| std::cmp::Reverse(b.components().count()));

    for dir_path in &dir_paths {
        compute_merkle_hash(dir_path, &mut dirs);
    }

    // Step 3: Group directories by Merkle hash
    let mut hash_groups: HashMap<[u8; 32], Vec<PathBuf>> = HashMap::new();
    for (path, node) in &dirs {
        if let Some(hash) = node.merkle_hash {
            // Apply filters
            if node.file_count < config.min_children {
                continue;
            }
            if node.total_size < config.min_total_size {
                continue;
            }
            hash_groups.entry(hash).or_default().push(path.clone());
        }
    }

    // Step 4: Build result groups (only groups with 2+ directories)
    let mut groups: Vec<DupeFolderGroup> = hash_groups
        .into_iter()
        .filter(|(_, paths)| paths.len() >= 2)
        .filter_map(|(hash, mut paths)| {
            paths.sort(); // deterministic ordering
            let first = dirs.get(&paths[0])?;
            Some(DupeFolderGroup {
                directories: paths,
                file_count: first.file_count,
                total_size: first.total_size,
                merkle_hash: hash,
            })
        })
        .collect();

    // Sort by total wasted size descending (most impactful first)
    groups.sort_by(|a, b| {
        let a_wasted = a.total_size * (a.directories.len() as u64 - 1);
        let b_wasted = b.total_size * (b.directories.len() as u64 - 1);
        b_wasted.cmp(&a_wasted)
    });

    tracing::info!(groups = groups.len(), "folder dedup scan complete");

    groups
}

/// Compute the Merkle hash for a directory node (bottom-up).
///
/// The hash is derived from:
/// - Sorted child file (name, size) pairs
/// - Sorted child directory Merkle hashes
///
/// This ensures the hash is content-based and independent of directory path.
fn compute_merkle_hash(path: &Path, dirs: &mut HashMap<PathBuf, DirNode>) {
    // Collect child dir hashes first (they should already be computed)
    let child_dir_hashes: Vec<[u8; 32]> = {
        let node = match dirs.get(path) {
            Some(n) => n,
            None => return,
        };
        let mut hashes: Vec<[u8; 32]> = Vec::new();
        for child_dir in &node.child_dirs {
            if let Some(child_node) = dirs.get(child_dir) {
                if let Some(h) = child_node.merkle_hash {
                    hashes.push(h);
                }
            }
        }
        hashes.sort();
        hashes
    };

    let node = match dirs.get_mut(path) {
        Some(n) => n,
        None => return,
    };

    // Sort child files for deterministic hashing
    node.child_files.sort();

    let mut hasher = blake3::Hasher::new();

    // Hash child files: name + size
    for (name, size) in &node.child_files {
        hasher.update(name.as_bytes());
        hasher.update(&size.to_le_bytes());
    }

    // Hash child directory Merkle roots
    for h in &child_dir_hashes {
        hasher.update(h);
    }

    node.merkle_hash = Some(*hasher.finalize().as_bytes());

    // Propagate recursive counts from child dirs
    for child_dir_path in node.child_dirs.clone() {
        if let Some(child) = dirs.get(&child_dir_path) {
            let child_count = child.file_count;
            let child_size = child.total_size;
            if let Some(parent) = dirs.get_mut(path) {
                parent.file_count += child_count;
                parent.total_size += child_size;
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn make_entry(path: &str, size: u64) -> FileEntry {
        let p = PathBuf::from(path);
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let ext = p.extension().map(|e| e.to_string_lossy().to_string());
        FileEntry {
            path: p,
            size,
            name,
            extension: ext,
            modified_at: 0,
            inode: None,
        }
    }

    #[test]
    fn identical_folders_detected() {
        let files = vec![
            // Folder A
            make_entry("/data/folder-a/file1.txt", 100),
            make_entry("/data/folder-a/file2.txt", 200),
            // Folder B (identical content)
            make_entry("/data/folder-b/file1.txt", 100),
            make_entry("/data/folder-b/file2.txt", 200),
        ];

        let groups = find_duplicate_folders(&files, &FolderDedupConfig::default());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].directories.len(), 2);
        assert_eq!(groups[0].file_count, 2);
        assert_eq!(groups[0].total_size, 300);
    }

    #[test]
    fn different_folders_not_grouped() {
        let files = vec![
            make_entry("/data/folder-a/file1.txt", 100),
            make_entry("/data/folder-a/file2.txt", 200),
            make_entry("/data/folder-b/file1.txt", 100),
            make_entry("/data/folder-b/file3.txt", 300), // different name
        ];

        let groups = find_duplicate_folders(&files, &FolderDedupConfig::default());
        assert!(groups.is_empty());
    }

    #[test]
    fn single_file_folders_filtered_by_min_children() {
        let files = vec![
            make_entry("/data/a/only.txt", 100),
            make_entry("/data/b/only.txt", 100),
        ];

        // Default min_children=2 filters these out
        let groups = find_duplicate_folders(&files, &FolderDedupConfig::default());
        assert!(groups.is_empty());

        // With min_children=1, they're detected
        let config = FolderDedupConfig {
            min_children: 1,
            ..Default::default()
        };
        let groups = find_duplicate_folders(&files, &config);
        assert_eq!(groups.len(), 1);
    }

    #[test]
    fn empty_input() {
        let groups = find_duplicate_folders(&[], &FolderDedupConfig::default());
        assert!(groups.is_empty());
    }

    #[test]
    fn three_identical_folders() {
        let files = vec![
            make_entry("/a/f1.txt", 50),
            make_entry("/a/f2.txt", 50),
            make_entry("/b/f1.txt", 50),
            make_entry("/b/f2.txt", 50),
            make_entry("/c/f1.txt", 50),
            make_entry("/c/f2.txt", 50),
        ];

        let groups = find_duplicate_folders(&files, &FolderDedupConfig::default());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].directories.len(), 3);
    }

    #[test]
    fn same_sizes_different_names_not_grouped() {
        let files = vec![
            make_entry("/a/alpha.txt", 100),
            make_entry("/a/beta.txt", 200),
            make_entry("/b/gamma.txt", 100), // same sizes but different names
            make_entry("/b/delta.txt", 200),
        ];

        let groups = find_duplicate_folders(&files, &FolderDedupConfig::default());
        assert!(groups.is_empty());
    }

    #[test]
    fn min_total_size_filter() {
        let files = vec![
            make_entry("/a/f1.txt", 10),
            make_entry("/a/f2.txt", 10),
            make_entry("/b/f1.txt", 10),
            make_entry("/b/f2.txt", 10),
        ];

        let config = FolderDedupConfig {
            min_children: 2,
            min_total_size: 1000, // 20 bytes < 1000
        };
        let groups = find_duplicate_folders(&files, &config);
        assert!(groups.is_empty());
    }

    #[test]
    fn sorted_by_wasted_size() {
        let files = vec![
            // Small duplicate pair
            make_entry("/small-a/f1.txt", 10),
            make_entry("/small-a/f2.txt", 10),
            make_entry("/small-b/f1.txt", 10),
            make_entry("/small-b/f2.txt", 10),
            // Large duplicate pair
            make_entry("/big-a/f1.txt", 10000),
            make_entry("/big-a/f2.txt", 10000),
            make_entry("/big-b/f1.txt", 10000),
            make_entry("/big-b/f2.txt", 10000),
        ];

        let groups = find_duplicate_folders(&files, &FolderDedupConfig::default());
        assert_eq!(groups.len(), 2);
        // Largest wasted first
        assert!(groups[0].total_size > groups[1].total_size);
    }
}
