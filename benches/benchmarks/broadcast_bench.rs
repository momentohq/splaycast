use std::sync::Arc;

use criterion::{criterion_group, measurement::WallTime, BenchmarkGroup, Criterion};
use tokio::sync::{broadcast, Semaphore};

use super::{bench_multithread_async, quick_test, BroadcastSender, Config};

pub fn broadcast(group: &mut BenchmarkGroup<'_, WallTime>, configs: Vec<Config>) {
    for config in configs {
        bench_multithread_async(
            "broadcast",
            group,
            config,
            || broadcast::channel(128).0,
            receiver_loop,
        );
    }
}

impl<T> BroadcastSender<T, broadcast::Receiver<T>> for broadcast::Sender<T> {
    fn send(&self, item: T) {
        match self.send(item) {
            Ok(_) => (),
            Err(_) => panic!("send should work"),
        }
    }

    fn subscribe(&self) -> broadcast::Receiver<T> {
        self.subscribe()
    }
}

async fn receiver_loop(mut receiver: broadcast::Receiver<Arc<Semaphore>>) {
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
}

fn only_broadcast(c: &mut Criterion) {
    broadcast(&mut c.benchmark_group("solo"), quick_test(6))
}

criterion_group!(benches, only_broadcast);
