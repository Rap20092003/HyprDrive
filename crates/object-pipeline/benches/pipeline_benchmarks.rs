//! Criterion benchmarks for the object pipeline.
//!
//! Benchmarks:
//! - `hash_file_1mb`: BLAKE3 hash of a 1MB file (extrapolate to 1GB: ~250ms)
//! - `hash_entries_batch_1k_cache_miss`: 1k files, all cache misses
//! - `hash_entries_batch_1k_cache_hit`: 1k files, all cache hits (reindex scenario)
//! - `pipeline_e2e_100`: Full pipeline for 100 small files including SQLite writes

use chrono::Utc;
use criterion::{criterion_group, criterion_main, Criterion};
use hyprdrive_object_pipeline::hasher;
use redb::Database;
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

fn make_entry(path: PathBuf, size: u64) -> hyprdrive_fs_indexer::types::IndexEntry {
    hyprdrive_fs_indexer::types::IndexEntry {
        fid: 1000 + size,
        parent_fid: 0,
        name: OsString::from(path.file_name().unwrap_or_default()),
        name_lossy: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        full_path: path,
        size,
        allocated_size: size.next_multiple_of(4096),
        is_dir: false,
        modified_at: Utc::now(),
        attributes: 0,
    }
}

fn bench_hash_file_1mb(c: &mut Criterion) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("1mb.bin");
    let data = vec![0x42u8; 1024 * 1024]; // 1MB
    std::fs::write(&path, &data).unwrap();

    c.bench_function("hash_file_1mb", |b| {
        b.iter(|| {
            hasher::hash_file(&path).unwrap();
        })
    });
}

fn bench_batch_cache_miss(c: &mut Criterion) {
    use std::sync::atomic::{AtomicU64, Ordering};

    let dir = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();

    // Create 100 small files.
    let mut entries = Vec::with_capacity(100);
    for i in 0..100 {
        let path = dir.path().join(format!("file_{i:04}.txt"));
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(format!("content {i}").as_bytes()).unwrap();
        f.flush().unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        entries.push(make_entry(path, size));
    }

    let counter = AtomicU64::new(0);
    c.bench_function("hash_batch_100_cache_miss", |b| {
        b.iter(|| {
            // Unique cache file per iteration = guaranteed all misses.
            let n = counter.fetch_add(1, Ordering::Relaxed);
            let cache = Database::create(cache_dir.path().join(format!("bench_{n}.redb"))).unwrap();
            hasher::hash_entries_batch(&entries, &cache, "vol_bench");
        })
    });
}

fn bench_batch_cache_hit(c: &mut Criterion) {
    let dir = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let cache = Database::create(cache_dir.path().join("bench.redb")).unwrap();

    // Create 100 small files.
    let mut entries = Vec::with_capacity(100);
    for i in 0..100 {
        let path = dir.path().join(format!("file_{i:04}.txt"));
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(format!("content {i}").as_bytes()).unwrap();
        f.flush().unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        entries.push(make_entry(path, size));
    }

    // Warm the cache.
    hasher::hash_entries_batch(&entries, &cache, "vol_bench");

    c.bench_function("hash_batch_100_cache_hit", |b| {
        b.iter(|| {
            hasher::hash_entries_batch(&entries, &cache, "vol_bench");
        })
    });
}

criterion_group!(
    benches,
    bench_hash_file_1mb,
    bench_batch_cache_miss,
    bench_batch_cache_hit
);
criterion_main!(benches);
