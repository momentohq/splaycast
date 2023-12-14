use std::{
    fmt::Display,
    sync::Arc,
    time::{Duration, Instant},
};

use criterion::{criterion_group, measurement::WallTime, BenchmarkGroup, BenchmarkId, Criterion};
use futures::Future;
use tokio::sync::Semaphore;

pub mod broadcast_bench;
pub mod splaycast_channel_bench;

fn compare_cast(c: &mut Criterion) {
    let mut group = c.benchmark_group("cast_comparison");

    for threads in [4, 8, 16, 32] {
        let configs = long_test(threads);
        broadcast_bench::broadcast(&mut group, configs.clone());
        splaycast_channel_bench::splaycast(&mut group, configs);
    }
}

pub fn quick_test(threads: usize) -> Vec<Config> {
    vec![
        Config {
            threads,
            subscribers: 1,
            queue_depth: 1,
        },
        Config {
            threads,
            subscribers: 100,
            queue_depth: 1,
        },
        Config {
            threads,
            subscribers: 1000,
            queue_depth: 1,
        },
        Config {
            threads,
            subscribers: 40000,
            queue_depth: 1,
        },
    ]
}

pub fn long_test(threads: usize) -> Vec<Config> {
    (0..11)
        .map(|i| Config {
            threads,
            subscribers: 2_usize.pow(i),
            queue_depth: 4,
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct Config {
    threads: usize,
    subscribers: usize,
    queue_depth: usize,
}

impl Display for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:.>2}/{:.>7}/{}d",
            self.threads, self.subscribers, self.queue_depth
        )
    }
}

pub trait BroadcastSender<T, TReceiver> {
    fn send(&mut self, item: T);
    fn subscribe(&self) -> TReceiver;
}

#[allow(clippy::expect_used)] // it is a benchmark, it's fine
fn bench_multithread_async<
    Receiver,
    Sender: BroadcastSender<Arc<Semaphore>, Receiver> + Send + 'static,
    FnReceiverFuture: Future<Output = ()> + Send + 'static,
>(
    name: &'static str,
    group: &mut BenchmarkGroup<'_, WallTime>,
    config: Config,
    get_sender: impl Fn() -> Sender + Copy,
    receiver_loop: impl Fn(Receiver) -> FnReceiverFuture + Copy,
) {
    group.throughput(criterion::Throughput::Elements(
        (config.subscribers * config.queue_depth) as u64,
    ));
    group.bench_function(BenchmarkId::new(name, config.clone()), |bencher| {
        let mut bencher = bencher.to_async(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(config.threads)
                .enable_all()
                .build()
                .expect("can make a tokio runtime"),
        );
        bencher.iter_custom(|iterations| async move {
            let mut sender = get_sender();

            for _ in 0..config.subscribers {
                let receiver = sender.subscribe();
                tokio::spawn(receiver_loop(receiver));
            }

            // We're not testing subscribe instant - we're testing signaling rate
            tokio::time::sleep(Duration::from_millis(100)).await;

            let start = Instant::now();
            tokio::spawn(async move {
                for _i in 0..iterations {
                    let semaphore = Arc::new(tokio::sync::Semaphore::new(0));
                    for _ in 0..config.queue_depth {
                        sender.send(semaphore.clone());
                    }
                    let _permit = semaphore
                        .acquire_many((config.subscribers * config.queue_depth) as u32)
                        .await
                        .expect("I should be able to acquire subscribers");
                }
            })
            .await
            .expect("it works");

            start.elapsed()
        });
    });
}

criterion_group!(comparison, compare_cast);
