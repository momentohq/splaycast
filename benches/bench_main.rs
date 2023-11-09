use criterion::criterion_main;

mod benchmarks;

criterion_main! {
    benchmarks::broadcast_bench::benches,
    benchmarks::splaycast_bench::benches,
    benchmarks::comparison,
}
