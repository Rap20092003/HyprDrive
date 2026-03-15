//! Benchmarks for the fs-indexer crate.
//!
//! These benchmarks test the non-OS-dependent algorithmic components.
//! MFT enumeration and enrichment require admin + NTFS, so they use
//! synthetic data for in-CI benchmarks, with `#[ignore]`d variants
//! that hit real volumes.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use hyprdrive_fs_indexer::types::TopoEntry;
use std::collections::HashMap;
use std::ffi::OsString;

/// Generate N synthetic topology entries forming a realistic directory tree.
///
/// Structure: root (fid=5) → 100 directories → files spread across them.
fn generate_topo_entries(n: usize) -> Vec<TopoEntry> {
    let mut entries = Vec::with_capacity(n);
    let dir_count = (n / 100).max(1); // ~1% directories

    // Root entry
    entries.push(TopoEntry {
        fid: 5,
        parent_fid: 5,
        name: OsString::from(""),
        is_dir: true,
        attributes: 0x10,
    });

    // Directories
    for i in 0..dir_count {
        let fid = 100 + i as u64;
        // Nest some dirs under each other for realistic path depth
        let parent = if i < 10 { 5 } else { 100 + (i % 10) as u64 };
        entries.push(TopoEntry {
            fid,
            parent_fid: parent,
            name: OsString::from(format!("dir_{i}")),
            is_dir: true,
            attributes: 0x10,
        });
    }

    // Files
    for i in 0..(n - dir_count - 1) {
        let fid = 10_000 + i as u64;
        let parent = 100 + (i % dir_count) as u64;
        entries.push(TopoEntry {
            fid,
            parent_fid: parent,
            name: OsString::from(format!("file_{i}.dat")),
            is_dir: false,
            attributes: 0x20, // FILE_ATTRIBUTE_ARCHIVE
        });
    }

    entries
}

/// Benchmark: build_parent_map from 100k topology entries.
/// Target: < 50ms (pure HashMap construction).
fn bench_build_parent_map_100k(c: &mut Criterion) {
    let entries = generate_topo_entries(100_000);

    c.bench_function("build_parent_map_100k", |b| {
        b.iter(|| {
            let map: HashMap<u64, (u64, OsString)> = black_box(&entries)
                .iter()
                .map(|e| (e.fid, (e.parent_fid, e.name.clone())))
                .collect();
            black_box(map);
        });
    });
}

/// Benchmark: reconstruct_path for 10k entries using a 100k parent map.
/// Target: < 100ms (chain walking with HashMap lookups).
fn bench_reconstruct_paths_10k(c: &mut Criterion) {
    let entries = generate_topo_entries(100_000);
    let parent_map: HashMap<u64, (u64, OsString)> = entries
        .iter()
        .map(|e| (e.fid, (e.parent_fid, e.name.clone())))
        .collect();

    let volume_root = std::path::Path::new("C:\\");

    // Pick 10k file fids to reconstruct
    let fids: Vec<u64> = entries
        .iter()
        .filter(|e| !e.is_dir)
        .take(10_000)
        .map(|e| e.fid)
        .collect();

    c.bench_function("reconstruct_paths_10k", |b| {
        b.iter(|| {
            for &fid in black_box(&fids) {
                let mut components = Vec::new();
                let mut current = fid;
                for _ in 0..4096 {
                    match parent_map.get(&current) {
                        Some((parent_fid, name)) => {
                            components.push(name.clone());
                            if *parent_fid == current {
                                break;
                            }
                            current = *parent_fid;
                        }
                        None => break,
                    }
                }
                components.reverse();
                let mut path = volume_root.to_path_buf();
                for component in &components {
                    path.push(component);
                }
                black_box(path);
            }
        });
    });
}

/// Benchmark: full topology-to-IndexEntry conversion pipeline (no I/O).
/// Simulates the map + path reconstruction done after MFT enumeration.
/// Target: < 500ms for 100k entries.
fn bench_topo_to_index_entries_100k(c: &mut Criterion) {
    use chrono::Utc;
    use hyprdrive_fs_indexer::types::IndexEntry;

    let topo_entries = generate_topo_entries(100_000);
    let parent_map: HashMap<u64, (u64, OsString)> = topo_entries
        .iter()
        .map(|e| (e.fid, (e.parent_fid, e.name.clone())))
        .collect();
    let volume = std::path::Path::new("C:\\");

    c.bench_function("topo_to_index_entries_100k", |b| {
        b.iter(|| {
            let entries: Vec<IndexEntry> = black_box(&topo_entries)
                .iter()
                .map(|topo| {
                    // Reconstruct path inline
                    let mut components = Vec::new();
                    let mut current = topo.fid;
                    for _ in 0..4096 {
                        match parent_map.get(&current) {
                            Some((parent_fid, name)) => {
                                components.push(name.clone());
                                if *parent_fid == current {
                                    break;
                                }
                                current = *parent_fid;
                            }
                            None => break,
                        }
                    }
                    components.reverse();
                    let mut full_path = volume.to_path_buf();
                    for component in &components {
                        full_path.push(component);
                    }
                    let name_lossy = topo.name.to_string_lossy().to_string();

                    IndexEntry {
                        fid: topo.fid,
                        parent_fid: topo.parent_fid,
                        name: topo.name.clone(),
                        name_lossy,
                        full_path,
                        size: 0,
                        allocated_size: 0,
                        is_dir: topo.is_dir,
                        modified_at: Utc::now(),
                        attributes: topo.attributes,
                    }
                })
                .collect();
            black_box(entries);
        });
    });
}

/// Benchmark: Vec<IndexEntry> allocation + push for 100k entries.
/// Measures baseline allocation cost separate from path reconstruction.
fn bench_index_entry_alloc_100k(c: &mut Criterion) {
    use chrono::Utc;
    use hyprdrive_fs_indexer::types::IndexEntry;
    use std::path::PathBuf;

    c.bench_function("index_entry_alloc_100k", |b| {
        b.iter(|| {
            let mut entries = Vec::with_capacity(100_000);
            for i in 0u64..100_000 {
                entries.push(IndexEntry {
                    fid: i,
                    parent_fid: i.saturating_sub(1),
                    name: OsString::from(format!("file_{i}.dat")),
                    name_lossy: format!("file_{i}.dat"),
                    full_path: PathBuf::from(format!("C:\\dir\\file_{i}.dat")),
                    size: i * 1024,
                    allocated_size: ((i * 1024 + 4095) / 4096) * 4096,
                    is_dir: false,
                    modified_at: Utc::now(),
                    attributes: 0x20,
                });
            }
            black_box(entries);
        });
    });
}

criterion_group!(
    benches,
    bench_build_parent_map_100k,
    bench_reconstruct_paths_10k,
    bench_topo_to_index_entries_100k,
    bench_index_entry_alloc_100k,
);
criterion_main!(benches);
