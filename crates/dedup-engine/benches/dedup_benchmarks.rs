//! Benchmarks for the dedup engine.

use criterion::{criterion_group, criterion_main, Criterion};
use hyprdrive_dedup_engine::hasher;
use hyprdrive_dedup_engine::scanner::group_by_size;
use hyprdrive_dedup_engine::FileEntry;
use std::io::Write;
use std::path::PathBuf;

fn bench_partial_hash_4kb(c: &mut Criterion) {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("4kb.bin");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(&vec![42u8; 4096]).unwrap();
    f.flush().unwrap();

    c.bench_function("partial_hash_4kb", |b| {
        b.iter(|| hasher::partial_hash(&path).unwrap())
    });
}

fn bench_full_hash_1mb(c: &mut Criterion) {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("1mb.bin");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(&vec![42u8; 1024 * 1024]).unwrap();
    f.flush().unwrap();

    c.bench_function("full_hash_1mb", |b| {
        b.iter(|| hasher::full_hash(&path).unwrap())
    });
}

fn bench_size_bucket_100k(c: &mut Criterion) {
    // Generate 100k synthetic FileEntry values
    let files: Vec<FileEntry> = (0..100_000)
        .map(|i| FileEntry {
            path: PathBuf::from(format!("/test/file_{i}.txt")),
            size: (i % 1000) as u64 * 100, // many collisions
            name: format!("file_{i}.txt"),
            extension: Some("txt".to_string()),
            modified_at: 0,
            inode: None,
        })
        .collect();

    c.bench_function("size_bucket_100k", |b| {
        b.iter(|| group_by_size(&files, 1))
    });
}

fn bench_fuzzy_match_1k(c: &mut Criterion) {
    let files: Vec<FileEntry> = (0..1_000)
        .map(|i| FileEntry {
            path: PathBuf::from(format!("/test/document_{i}.pdf")),
            size: 100,
            name: format!("document_{i}.pdf"),
            extension: Some("pdf".to_string()),
            modified_at: 0,
            inode: None,
        })
        .collect();

    c.bench_function("fuzzy_match_1k", |b| {
        b.iter(|| hyprdrive_dedup_engine::find_similar_names(&files, 0.85))
    });
}

criterion_group!(
    benches,
    bench_partial_hash_4kb,
    bench_full_hash_1mb,
    bench_size_bucket_100k,
    bench_fuzzy_match_1k,
);
criterion_main!(benches);
