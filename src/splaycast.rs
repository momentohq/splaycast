use std::sync::Arc;

use crate::{
    shared::Shared, splaycast_engine::SplaycastEngine, splaycast_receiver::SplaycastReceiver,
};

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
    pub fn new<Upstream>(
        upstream: Upstream,
        buffer_size: usize,
    ) -> (SplaycastEngine<Upstream, Item>, Self)
    where
        Upstream: futures::Stream<Item = Item> + Unpin,
    {
        let shared = Arc::new(Shared::new(buffer_size));
        let engine = SplaycastEngine::new(upstream, shared.clone());
        (engine, Self { shared })
    }

    pub fn subscribe(&self) -> SplaycastReceiver<Item> {
        SplaycastReceiver::new(self.shared.clone())
    }

    pub fn subscriber_count(&self) -> usize {
        self.shared.subscriber_count()
    }
}
