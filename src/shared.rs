use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    task::Context,
};

use arc_swap::ArcSwap;
use crossbeam_queue::SegQueue;
use futures::task::AtomicWaker;

use crate::SplaycastEntry;

/// Shared, lock-free state for splaying out notifications to receiver streams from an upstream stream.
pub struct Shared<Item> {
    subscriber_count: AtomicUsize,
    wakers: Arc<SegQueue<WakeHandle>>,
    queue: Arc<ArcSwap<VecDeque<SplaycastEntry<Item>>>>,
    waker: AtomicWaker,
    is_dead: AtomicBool,
}

impl<Item> std::fmt::Debug for Shared<Item>
where
    Item: Clone,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shared")
            .field("subscriber_count", &self.subscriber_count)
            .finish()
    }
}

impl<Item> Shared<Item>
where
    Item: Clone,
{
    pub fn new(buffer_size: usize) -> Self {
        Self {
            subscriber_count: Default::default(),
            wakers: Default::default(),
            queue: Arc::new(ArcSwap::from_pointee(VecDeque::with_capacity(buffer_size))),
            waker: Default::default(),
            is_dead: Default::default(),
        }
    }

    pub fn set_dead(&self) {
        self.is_dead.store(true, Ordering::Release);
        self.waker.wake(); // Make sure the Engine runs promptly
    }

    pub fn is_dead(&self) -> bool {
        self.is_dead.load(Ordering::Acquire)
    }

    #[inline]
    pub fn subscriber_count(&self) -> usize {
        self.subscriber_count.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn increment_subscriber_count(&self) -> usize {
        let count = self.subscriber_count.fetch_add(1, Ordering::Relaxed) + 1;
        log::trace!("incrementing subscriber count to {count}");
        count
    }

    #[inline]
    pub fn decrement_subscriber_count(&self) -> usize {
        let count = self.subscriber_count.fetch_sub(1, Ordering::Relaxed) - 1;
        log::trace!("decrementing subscriber count to {count}");
        count
    }

    #[inline]
    pub(crate) fn load_queue(&self) -> arc_swap::Guard<Arc<VecDeque<SplaycastEntry<Item>>>> {
        self.queue.load()
    }

    #[inline]
    pub(crate) fn swap_queue(
        &self,
        next: VecDeque<SplaycastEntry<Item>>,
    ) -> Arc<VecDeque<SplaycastEntry<Item>>> {
        log::trace!(
            "swap queue length {} -> {}",
            self.queue.load().len(),
            next.len()
        );
        self.queue.swap(Arc::new(next))
    }

    #[inline]
    pub fn register_waker(&self, handle: WakeHandle) {
        log::trace!("register waker at {}", handle.message_id);
        if self.is_dead() {
            handle.wake();
            return;
        }
        self.wakers.push(handle);
        self.waker.wake()
    }

    #[inline]
    pub fn register_wake_interest(&self, context: &mut Context) {
        self.waker.register(context.waker());
    }

    #[inline]
    pub fn drain_wakelist(self: &Arc<Self>) -> impl Iterator<Item = WakeHandle> {
        WakeIterator {
            shared: self.clone(),
        }
    }
}

struct WakeIterator<T>
where
    T: Clone,
{
    shared: Arc<Shared<T>>,
}
impl<T: Clone> Iterator for WakeIterator<T> {
    type Item = WakeHandle;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.shared.wakers.pop()
    }
}

#[derive(Debug)]
pub struct WakeHandle {
    message_id: u64,
    waker: core::task::Waker,
    this_handle_woke: Arc<AtomicBool>,
}

impl WakeHandle {
    pub fn new(
        message_id: u64,
        waker: core::task::Waker,
        this_handle_woke: Arc<AtomicBool>,
    ) -> Self {
        Self {
            message_id,
            waker,
            this_handle_woke,
        }
    }

    #[inline]
    pub fn wake(self) {
        self.this_handle_woke.store(true, Ordering::Release);
        self.waker.wake()
    }

    #[inline]
    pub fn next_message_id(&self) -> u64 {
        self.message_id
    }
}
