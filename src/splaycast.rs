use std::sync::Arc;

use crate::{engine::Engine, receiver::Receiver, shared::Shared};

/// The handle for attaching new subscribers to and inspecting the state of a splaycast.
#[derive(Debug)]
pub struct Splaycast<Item>
where
    Item: Clone,
{
    shared: Arc<Shared<Item>>,
}

impl<Item> Splaycast<Item>
where
    Item: Unpin + Clone + Send,
{
    // Wire a splaying channel adapter to an upstream stream.
    pub(crate) fn new<Upstream>(
        upstream: Upstream,
        buffer_size: usize,
    ) -> (Engine<Upstream, Item>, Self)
    where
        Upstream: futures::Stream<Item = Item> + Unpin,
    {
        let shared = Arc::new(Shared::new(buffer_size));
        let engine = Engine::new(upstream, shared.clone());
        (engine, Self { shared })
    }

    /// Get a new streaming Receiver from the upstream stream. Values are cloned to
    /// this receiver, and lag is tracked if you consume too slowly and fall off of
    /// the configured buffer.
    pub fn subscribe(&self) -> Receiver<Item> {
        Receiver::new(self.shared.clone())
    }

    /// This is informational, and may be stale before it even returns. It is maintained
    /// as a ~best~ reasonable-effort counter that tracks subscribers. Memory ordering is
    /// Relaxed, but it should settle within a _very_ short window of time to the actual
    /// value in practice. But concurrent subscribe/drop will return you a snapshot of
    /// an instant from this method, not necessarily the same instant that you inspect the
    /// return value though.
    pub fn subscriber_count(&self) -> usize {
        self.shared.subscriber_count()
    }
}
