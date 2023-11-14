use std::sync::Arc;

use criterion::{criterion_group, measurement::WallTime, BenchmarkGroup, Criterion};
use futures::StreamExt;
use splaycast::Splaycast;
use tokio::sync::{
    mpsc::{unbounded_channel, UnboundedSender},
    Semaphore,
};
use tokio_stream::wrappers::UnboundedReceiverStream;

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
        match self.publish_handle.send(item) {
            Ok(_) => (),
            Err(_) => panic!("send should not fail"),
        }
    }

    fn subscribe(&self) -> splaycast::Receiver<Arc<Semaphore>> {
        self.splaycast.subscribe()
    }
}

/// Splaycast is a direct stream adapter - so we have to plug in a stream
struct BenchmarkStreamAdapter {
    splaycast: Splaycast<Arc<Semaphore>>,
    publish_handle: UnboundedSender<Arc<Semaphore>>,
}

fn get_splaycast() -> BenchmarkStreamAdapter {
    let (publish_handle, upstream) = unbounded_channel::<Arc<tokio::sync::Semaphore>>();
    let upstream = UnboundedReceiverStream::new(upstream); // multiple layers of wrapping here. It's best if you have an inbound native stream, like from Tonic
    let (engine, splaycast) = splaycast::wrap(upstream, 16);

    // This is the 3rd component of a splaycast, beyond the idea of a sender and a receiver.
    tokio::spawn(engine);

    BenchmarkStreamAdapter {
        splaycast,
        publish_handle,
    }
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
