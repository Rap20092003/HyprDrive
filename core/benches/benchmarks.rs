use criterion::{criterion_group, criterion_main, Criterion};
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

criterion_group!(
    benches,
    bench_filter_compile,
    bench_vector_clock_merge,
    bench_undo_stack
);
criterion_main!(benches);
