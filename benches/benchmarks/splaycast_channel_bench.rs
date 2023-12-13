use std::sync::Arc;

use criterion::{criterion_group, measurement::WallTime, BenchmarkGroup, Criterion};
use futures::StreamExt;
use splaycast::{Sender, Splaycast};
use tokio::sync::Semaphore;

use super::{bench_multithread_async, quick_test, BroadcastSender, Config};

pub fn splaycast(group: &mut BenchmarkGroup<'_, WallTime>, configs: Vec<Config>) {
    for config in configs {
        bench_multithread_async("splaycast", group, config, get_splaycast, receiver_loop);
    }
}

impl BroadcastSender<Arc<Semaphore>, splaycast::Receiver<Arc<Semaphore>>>
    for BenchmarkStreamAdapter
{
    fn send(&self, item: Arc<Semaphore>) {
        match self.sender.send(item) {
            Ok(_) => (),
            Err(_) => panic!("send should not fail"),
        }
    }

    fn subscribe(&self) -> splaycast::Receiver<Arc<Semaphore>> {
        self.splaycast.subscribe()
    }
}

/// the benchmark uses the sender and the splaycast handle in the same place, so let's tie them together for convenience
///
/// Note that splaycast is intended to be used as a futures::Stream plugin. This benchmark is oriented toward a "channel"
/// rather than a "stream." This gives a pessimistic view of a splaycast used with a Stream upstream.
struct BenchmarkStreamAdapter {
    splaycast: Splaycast<Arc<Semaphore>>,
    sender: Sender<Arc<Semaphore>>,
}

fn get_splaycast() -> BenchmarkStreamAdapter {
    let (sender, engine, splaycast) = splaycast::channel(16);

    // This is the 3rd component of a splaycast, beyond the idea of a sender and a receiver.
    tokio::spawn(engine);

    BenchmarkStreamAdapter { sender, splaycast }
}

async fn receiver_loop(mut receiver: splaycast::Receiver<Arc<Semaphore>>) {
    while let Some(message) = receiver.next().await {
        match message {
            splaycast::Message::Entry { item } => {
                item.add_permits(1);
            }
            splaycast::Message::Lagged { count } => {
                eprintln!("lagged {count}")
            }
        }
    }
}

fn only_splaycast(c: &mut Criterion) {
    let _ = env_logger::builder().parse_default_env().try_init();
    splaycast(&mut c.benchmark_group("solo"), quick_test(16))
}

criterion_group!(benches, only_splaycast);
