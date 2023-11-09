use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use criterion::{criterion_group, measurement::WallTime, BenchmarkGroup, BenchmarkId, Criterion};
use futures::StreamExt;
use splaycast::Splaycast;
use tokio::sync::mpsc::unbounded_channel;
use tokio_stream::wrappers::UnboundedReceiverStream;

#[allow(clippy::expect_used)] // this is a benchmark, lints like this don't matter.
pub fn splaycast(group: &mut BenchmarkGroup<'_, WallTime>, sizes: Vec<usize>) {
    for subscribers in sizes {
        // for subscribers in [1, 10] {
        group.throughput(criterion::Throughput::Elements(subscribers as u64));

        group.bench_function(BenchmarkId::new("splaycast", subscribers), |bencher| {
            let mut bencher = bencher.to_async(
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(6)
                    .enable_all()
                    .build()
                    .expect("can make a tokio runtime"),
            );
            bencher.iter_custom(|iterations| async move {
                let (upstream_sender, upstream) =
                    unbounded_channel::<Arc<tokio::sync::Semaphore>>();
                let (splaycast_job, splaycast) =
                    Splaycast::new(UnboundedReceiverStream::new(upstream), 32);

                for _ in 0..subscribers {
                    let mut receiver = splaycast.subscribe();
                    tokio::spawn(async move {
                        loop {
                            match receiver.next().await {
                                Some(message) => match message {
                                    splaycast::SplaycastMessage::Entry { item } => {
                                        item.add_permits(1);
                                    }
                                    splaycast::SplaycastMessage::Lagged { count } => {
                                        eprintln!("lagged {count}")
                                    }
                                },
                                None => break,
                            }
                        }
                    });
                }

                while splaycast.subscriber_count() < subscribers {
                    // Subscribers aren't attached instantly, to allow the splay job to work without locking.
                    tokio::task::yield_now().await;
                    log::warn!(
                        "waiting for subscriber count {} < {}",
                        splaycast.subscriber_count(),
                        subscribers
                    )
                }

                log::debug!("waiting for subscribers to poll");
                tokio::time::sleep(Duration::from_millis(5)).await; // fixme: need to make the engine not vulnerable to subscribe races

                log::debug!("spawning splaycast job");
                tokio::spawn(splaycast_job); // Synchronous, lock-free context for handling subscribers and notifications.

                let start = Instant::now();
                tokio::spawn(async move {
                    for _ in 0..iterations {
                        let semaphore = Arc::new(tokio::sync::Semaphore::new(0));
                        let _ = upstream_sender.send(semaphore.clone());
                        let _permit = semaphore
                            .acquire_many(subscribers as u32)
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
}

fn only_splaycast(c: &mut Criterion) {
    let _ = env_logger::builder().parse_default_env().try_init();
    splaycast(&mut c.benchmark_group("solo"), vec![1, 100, 1000, 40000])
}

criterion_group!(benches, only_splaycast);
