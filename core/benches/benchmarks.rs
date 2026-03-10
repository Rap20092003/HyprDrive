use criterion::{criterion_group, criterion_main, Criterion};

fn bench_noop(c: &mut Criterion) {
    c.bench_function("noop", |b| {
        b.iter(|| {
            // Phase 0 stub — will be replaced with real benchmarks
            std::hint::black_box(42)
        })
    });
}

criterion_group!(benches, bench_noop);
criterion_main!(benches);
