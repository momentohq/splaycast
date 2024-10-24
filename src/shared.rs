use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
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
    next_receiver_id: AtomicU64,
    subscriber_count: Arc<AtomicUsize>,
    subscribe_sequence: AtomicU64,
    subscribe_tail_sequence: AtomicU64,
    wakers: Arc<SegQueue<(u64, WakeHandle)>>,
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
    pub fn new() -> Self {
        Self {
            next_receiver_id: Default::default(),
            subscriber_count: Default::default(),
            subscribe_sequence: AtomicU64::new(1),
            subscribe_tail_sequence: AtomicU64::new(1),
            wakers: Arc::new(SegQueue::new()),
            queue: Arc::new(ArcSwap::from_pointee(VecDeque::new())),
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

    pub fn next_receiver_id(&self) -> u64 {
        self.next_receiver_id.fetch_add(1, Ordering::Relaxed)
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
        let first_sequence_number = next.front().map(|item| item.id).unwrap_or(0);
        let last_sequence_number = next.back().map(|item| item.id).unwrap_or(0);
        let previous = self.queue.swap(Arc::new(next));
        self.subscribe_sequence
            .store(last_sequence_number + 1, Ordering::Relaxed);
        self.subscribe_tail_sequence
            .store(first_sequence_number + 1, Ordering::Release);
        previous
    }

    #[inline]
    pub(crate) fn subscribe_sequence_number(&self) -> u64 {
        self.subscribe_sequence.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn subscribe_tail_sequence_number(&self) -> u64 {
        self.subscribe_tail_sequence.load(Ordering::Acquire)
    }

    #[inline]
    pub fn register_waker(&self, receiver_id: u64, handle: WakeHandle) {
        log::trace!("register waker at {}", handle.message_id);
        if self.is_dead() {
            handle.wake();
            return;
        }
        self.wakers.push((receiver_id, handle));
        self.waker.wake()
    }

    #[inline]
    pub fn register_wake_interest(&self, context: &mut Context) {
        self.waker.register(context.waker());
    }

    #[inline]
    pub fn drain_wakelist(self: &Arc<Self>) -> impl Iterator<Item = (u64, WakeHandle)> {
        WakeIterator {
            shared: self.clone(),
        }
    }

    #[inline]
    pub fn subscriber_count_handle(&self) -> SubscriberCountHandle {
        SubscriberCountHandle {
            subscriber_count: Arc::downgrade(&self.subscriber_count),
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
    type Item = (u64, WakeHandle);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.shared.wakers.pop()
    }
}

#[derive(Debug)]
pub struct WakeHandle {
    message_id: u64,
    waker: core::task::Waker,
}

impl WakeHandle {
    pub fn new(message_id: u64, waker: core::task::Waker) -> Self {
        Self { message_id, waker }
    }

    #[inline]
    pub fn wake(self) {
        self.waker.wake()
    }

    #[inline]
    pub fn next_message_id(&self) -> u64 {
        self.message_id
    }

    #[inline]
    pub fn will_wake(&self, other: &Self) -> bool {
        self.waker.will_wake(&other.waker)
    }
}

/// A handle for inspecting the current subscriber count.
/// Subscriber counts are updated asynchronously, so values may be stale.
#[derive(Debug, Clone)]
pub struct SubscriberCountHandle {
    subscriber_count: std::sync::Weak<AtomicUsize>,
}

impl SubscriberCountHandle {
    /// Get the current subscriber count.
    /// Subscriber counts are updated asynchronously, so values may be stale.
    ///
    /// Returns None if the channel has been dropped.
    pub fn get(&self) -> Option<usize> {
        self.subscriber_count
            .upgrade()
            .map(|count| count.load(Ordering::Relaxed))
    }
}
