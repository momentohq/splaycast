use std::{sync::Arc, time::Instant};

use criterion::{criterion_group, measurement::WallTime, BenchmarkGroup, BenchmarkId, Criterion};
use tokio::sync::broadcast;

#[allow(clippy::expect_used)] // this is a benchmark, lints like this don't matter.
pub fn broadcast(group: &mut BenchmarkGroup<'_, WallTime>, sizes: Vec<usize>) {
    for subscribers in sizes {
        group.throughput(criterion::Throughput::Elements(subscribers as u64));

        group.bench_function(BenchmarkId::new("broadcast", subscribers), |bencher| {
            let mut bencher = bencher.to_async(
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(6)
                    .enable_all()
                    .build()
                    .expect("can make a tokio runtime"),
            );
            bencher.iter_custom(|iterations| async move {
                let (sender, receiver) = broadcast::channel::<Arc<tokio::sync::Semaphore>>(128);

                for _ in 0..subscribers {
                    let mut receiver = sender.subscribe();
                    tokio::spawn(async move {
                        loop {
                            match receiver.recv().await {
                                Ok(i) => {
                                    i.add_permits(1);
                                }
                                Err(e) => match e {
                                    broadcast::error::RecvError::Closed => break,
                                    broadcast::error::RecvError::Lagged(lagged) => {
                                        eprintln!("lagged {lagged}")
                                    }
                                },
                            }
                        }
                    });
                }
                drop(receiver);

                let start = Instant::now();
                tokio::spawn(async move {
                    for _i in 0..iterations {
                        let semaphore = Arc::new(tokio::sync::Semaphore::new(0));
                        let _ = sender.send(semaphore.clone());
                        let _permit = semaphore
                            .acquire_many(subscribers as u32)
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
}

fn only_broadcast(c: &mut Criterion) {
    broadcast(&mut c.benchmark_group("solo"), vec![1, 100, 1000, 40000])
}

criterion_group!(benches, only_broadcast);
