use std::sync::Arc;

use crate::{
    buffer_policy::BufferPolicy,
    engine::Engine,
    receiver::Receiver,
    shared::{Shared, SubscriberCountHandle},
};

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
    pub(crate) fn new<Upstream, Policy>(
        upstream: Upstream,
        buffer_policy: Policy,
    ) -> (Engine<Upstream, Item, Policy>, Self)
    where
        Upstream: futures::Stream<Item = Item> + Unpin,
        Policy: BufferPolicy<Item>,
    {
        let shared = Arc::new(Shared::new());
        let engine = Engine::new(upstream, shared.clone(), buffer_policy);
        (engine, Self { shared })
    }

    /// Get a new streaming Receiver from the upstream stream. Values are cloned to
    /// this receiver, and lag is tracked if you consume too slowly and fall off of
    /// the configured buffer.
    pub fn subscribe(&self) -> Receiver<Item> {
        Receiver::new(self.shared.clone())
    }

    /// Get a new streaming Receiver from the upstream stream. Values are cloned to
    /// this receiver, and lag is tracked if you consume too slowly and fall off of
    /// the configured buffer.
    ///
    /// This subscription receives starting from the oldest item in the buffer. You will
    /// race with the buffer policy to get the items, so you may see lag messages as you
    /// get started and catch up.
    pub fn subscribe_at_tail(&self) -> Receiver<Item> {
        Receiver::new_at_buffer_start(self.shared.clone())
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

    /// This is informational, and may be stale before it even returns. It is maintained
    /// as a ~best~ reasonable-effort counter that tracks subscribers. Memory ordering is
    /// Relaxed, but it should settle within a _very_ short window of time to the actual
    /// value in practice. But concurrent subscribe/drop will return you a snapshot of
    /// an instant from this method, not necessarily the same instant that you inspect the
    /// return value though.
    pub fn subscriber_count_handle(&self) -> SubscriberCountHandle {
        self.shared.subscriber_count_handle()
    }
}

impl<T: Clone> Drop for Splaycast<T> {
    fn drop(&mut self) {
        self.shared.set_dead()
    }
}
