use criterion::{criterion_group, Criterion};

pub mod broadcast_bench;
pub mod splaycast_bench;

fn compare_cast(c: &mut Criterion) {
    let mut group = c.benchmark_group("cast_comparison");
    let mut sizes = vec![1, 10, 100, 1000];
    for i in 0..20 {
        // 20 per 100k
        sizes.push(10000 + i * 5000)
    }
    broadcast_bench::broadcast(&mut group, sizes.clone());
    splaycast_bench::splaycast(&mut group, sizes);
}

criterion_group!(comparison, compare_cast);
