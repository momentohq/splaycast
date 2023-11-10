use std::{fmt::Display, sync::Arc, time::Instant};

use criterion::{criterion_group, Criterion, BenchmarkGroup, measurement::WallTime, BenchmarkId};
use futures::Future;
use tokio::sync::Semaphore;

pub mod broadcast_bench;
pub mod splaycast_bench;

fn compare_cast(c: &mut Criterion) {
    let mut group = c.benchmark_group("cast_comparison");

    for threads in [32] {
        let configs = long_test(threads);
        broadcast_bench::broadcast(&mut group, configs.clone());
        splaycast_bench::splaycast(&mut group, configs);
    }
}

pub fn quick_test(threads: usize) -> Vec<Config> {
    vec![
        Config {
            threads,
            subscribers: 1,
        },
        Config {
            threads,
            subscribers: 100,
        },
        Config {
            threads,
            subscribers: 1000,
        },
        Config {
            threads,
            subscribers: 40000,
        },
    ]
}

pub fn long_test(threads: usize) -> Vec<Config> {
    (0..20).map(|i| Config {
        threads,
        subscribers: 2_usize.pow(i),
    }).collect()
}

#[derive(Debug, Clone)]
pub struct Config {
    threads: usize,
    subscribers: usize,
}

impl Display for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.>2}/{:.>7}", self.threads, self.subscribers)
    }
}

pub trait BroadcastSender<T, TReceiver> {
    fn send(&self, item: T);
    fn subscribe(&self) -> TReceiver;
}

fn bench_multithread_async<Receiver, Sender: BroadcastSender<Arc<Semaphore>, Receiver> + Send + 'static, FnReceiverFuture: Future<Output = ()> + Send + 'static>(
    name: &'static str,
    group: &mut BenchmarkGroup<'_, WallTime>,
    config: Config,
    get_sender: impl Fn() -> Sender + Copy,
    receiver_loop: impl Fn(Receiver) -> FnReceiverFuture + Copy,
) {
    group.bench_function(BenchmarkId::new(name, config.clone()), |bencher| {
        let mut bencher = bencher.to_async(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(config.threads)
                .enable_all()
                .build()
                .expect("can make a tokio runtime"),
        );
        bencher.iter_custom(|iterations| async move {
            let sender = get_sender();

            for _ in 0..config.subscribers {
                let receiver = sender.subscribe();
                tokio::spawn(receiver_loop(receiver));
            }

            let start = Instant::now();
            tokio::spawn(async move {
                for _i in 0..iterations {
                    let semaphore = Arc::new(tokio::sync::Semaphore::new(0));
                    let _ = sender.send(semaphore.clone());
                    let _permit = semaphore
                        .acquire_many(config.subscribers as u32)
                        .await
                        .expect("I should be able to acquire subscribers");
                }
                drop(sender);
            })
            .await
            .expect("it works");

            start.elapsed()
        });
    });
}


criterion_group!(comparison, compare_cast);
