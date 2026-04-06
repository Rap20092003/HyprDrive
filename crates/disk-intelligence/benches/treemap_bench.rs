use criterion::{black_box, criterion_group, criterion_main, Criterion};
use hyprdrive_disk_intelligence::treemap::{squarify, Rect, TreemapItem};

fn bench_treemap_1m(c: &mut Criterion) {
    let items: Vec<TreemapItem> = (0..1_000_000u32)
        .map(|i| TreemapItem {
            id: i,
            weight: (1_000_000 - i) as f64 + 1.0,
        })
        .collect();
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        w: 1920.0,
        h: 1080.0,
    };

    c.bench_function("treemap_squarify_1M_nodes", |b| {
        b.iter(|| squarify(black_box(bounds), black_box(&items)))
    });
}

fn bench_treemap_10k(c: &mut Criterion) {
    let items: Vec<TreemapItem> = (0..10_000u32)
        .map(|i| TreemapItem {
            id: i,
            weight: (10_000 - i) as f64 + 1.0,
        })
        .collect();
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        w: 1920.0,
        h: 1080.0,
    };

    c.bench_function("treemap_squarify_10K_nodes", |b| {
        b.iter(|| squarify(black_box(bounds), black_box(&items)))
    });
}

criterion_group!(benches, bench_treemap_1m, bench_treemap_10k);
criterion_main!(benches);
