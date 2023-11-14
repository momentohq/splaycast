use std::sync::Arc;

use crate::{engine::Engine, receiver::Receiver, shared::Shared};

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

    pub fn subscribe(&self) -> Receiver<Item> {
        Receiver::new(self.shared.clone())
    }

    pub fn subscriber_count(&self) -> usize {
        self.shared.subscriber_count()
    }
}
