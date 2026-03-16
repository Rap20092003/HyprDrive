use criterion::{black_box, criterion_group, criterion_main, Criterion};
use hyprdrive_core::domain::{
    enums::FileCategory, filter::FilterExpr, id::DeviceId, sync::VectorClock, undo::UndoStack,
};

fn bench_filter_compile(c: &mut Criterion) {
    let filters = vec![
        FilterExpr::FileType(FileCategory::Video),
        FilterExpr::Extension("pdf".into()),
        FilterExpr::SizeRange {
            min: 0,
            max: u64::MAX,
        },
        FilterExpr::IsBuildArtifact,
        FilterExpr::Duplicate,
        FilterExpr::IsWasteful(0.5),
        FilterExpr::And(vec![
            FilterExpr::FileType(FileCategory::Image),
            FilterExpr::SizeRange {
                min: 1_000_000,
                max: u64::MAX,
            },
        ]),
        FilterExpr::Or(vec![
            FilterExpr::Extension("mp4".into()),
            FilterExpr::Extension("mkv".into()),
        ]),
        FilterExpr::Not(Box::new(FilterExpr::IsBuildArtifact)),
        FilterExpr::And(vec![
            FilterExpr::Or(vec![
                FilterExpr::Extension("pdf".into()),
                FilterExpr::Extension("doc".into()),
            ]),
            FilterExpr::Not(Box::new(FilterExpr::IsBuildArtifact)),
        ]),
    ];

    c.bench_function("filter_compile_10_variants", |b| {
        b.iter(|| {
            for f in &filters {
                let _ = f.compile_to_sql();
            }
        });
    });
}

fn bench_vector_clock_merge(c: &mut Criterion) {
    // Create two clocks with 100 devices each
    let devices: Vec<DeviceId> = (0..100).map(|_| DeviceId::new()).collect();

    let mut clock_a = VectorClock::new();
    let mut clock_b = VectorClock::new();

    for (i, d) in devices.iter().enumerate() {
        for _ in 0..(i % 10 + 1) {
            clock_a.increment(*d);
        }
        for _ in 0..((i + 5) % 10 + 1) {
            clock_b.increment(*d);
        }
    }

    c.bench_function("vector_clock_merge_100_devices", |b| {
        b.iter(|| {
            let mut c = clock_a.clone();
            c.merge(&clock_b);
        });
    });
}

fn bench_undo_stack(c: &mut Criterion) {
    use chrono::Utc;
    use hyprdrive_core::domain::undo::UndoEntry;

    let entry = UndoEntry {
        description: "Moved 5 files to Photos".into(),
        timestamp: Utc::now(),
        inverse_action: r#"{"action":"move","from":"/b","to":"/a"}"#.into(),
    };

    c.bench_function("undo_stack_push_pop_at_capacity", |b| {
        let mut stack = UndoStack::new();
        // Fill to capacity
        for _ in 0..50 {
            stack.push(entry.clone());
        }
        b.iter(|| {
            stack.push(entry.clone());
            let _ = stack.pop();
        });
    });
}

/// Benchmark: redb inode cache insert + lookup for 1M entries.
/// Target: < 1μs/lookup after population.
fn bench_redb_inode_lookup(c: &mut Criterion) {
    use hyprdrive_core::db::cache;
    let dir = tempfile::TempDir::new().expect("tempdir");
    let db = cache::open_cache(dir.path().join("bench.redb").as_path()).expect("open cache");

    // Pre-populate with 10k entries (full 1M takes too long for setup)
    {
        let txn = db.begin_write().expect("write txn");
        {
            let table_def: redb::TableDefinition<&str, &str> =
                redb::TableDefinition::new("inode_cache");
            let mut table = txn.open_table(table_def).expect("open table");
            for i in 0u64..10_000 {
                let key = format!("vol1:{i}:{}", 1_700_000_000i64 + i as i64);
                let val = format!("obj_{i:08x}");
                table.insert(key.as_str(), val.as_str()).expect("insert");
            }
        }
        txn.commit().expect("commit");
    }

    c.bench_function("redb_inode_lookup_10k", |b| {
        b.iter(|| {
            let key = cache::inode::cache_key("vol1", 5000, 1_700_005_000i64);
            let result = cache::inode::get(&db, &key).expect("get");
            black_box(result);
        });
    });

    // Batch lookup: 100 sequential lookups
    c.bench_function("redb_inode_batch_lookup_100", |b| {
        b.iter(|| {
            for i in 0u64..100 {
                let key =
                    cache::inode::cache_key("vol1", i * 100, 1_700_000_000i64 + i as i64 * 100);
                let result = cache::inode::get(&db, &key).expect("get");
                black_box(result);
            }
        });
    });
}

criterion_group!(
    benches,
    bench_filter_compile,
    bench_vector_clock_merge,
    bench_undo_stack,
    bench_redb_inode_lookup
);
criterion_main!(benches);
