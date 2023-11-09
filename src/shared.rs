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

pub struct Shared<Item>
where
    Item: Clone,
{
    subscriber_count: AtomicUsize,
    wakers: Arc<SegQueue<WakeHandle>>,
    queue: Arc<ArcSwap<VecDeque<SplaycastEntry<Item>>>>,
    wake_interest_sequence_number: AtomicU64,
    waker: AtomicWaker,
}

impl<Item> std::fmt::Debug for Shared<Item>
where
    Item: Clone,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shared")
            .field("subscriber_count", &self.subscriber_count)
            .field("wakers_count", &self.wakers.len())
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
            wake_interest_sequence_number: Default::default(),
            waker: Default::default(),
        }
    }

    pub fn subscriber_count(&self) -> usize {
        self.subscriber_count
            .load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn increment_subscriber_count(&self) -> usize {
        let count = self
            .subscriber_count
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
            + 1;
        log::trace!("incrementing subscriber count to {count}");
        count
    }

    pub fn decrement_subscriber_count(&self) -> usize {
        let count = self
            .subscriber_count
            .fetch_sub(1, std::sync::atomic::Ordering::AcqRel)
            - 1;
        log::trace!("decrementing subscriber count to {count}");
        count
    }

    pub(crate) fn load_queue(&self) -> arc_swap::Guard<Arc<VecDeque<SplaycastEntry<Item>>>> {
        self.queue.load()
    }

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

    pub fn register_waker(&self, handle: WakeHandle) {
        let message_id = handle.message_id;
        log::trace!("register waker at {message_id}");
        self.wakers.push(handle);
        let previous = self
            .wake_interest_sequence_number
            .fetch_min(message_id, Ordering::SeqCst);
        if message_id < previous {
            self.waker.wake()
        }
    }

    pub fn pop_waker(&self) -> Option<WakeHandle> {
        self.wakers.pop()
    }

    pub fn waiting(&self) -> usize {
        self.wakers.len()
    }

    pub fn load_and_reset_wake_interest(&self, context: &mut Context) -> Option<u64> {
        let stored = self
            .wake_interest_sequence_number
            .fetch_max(u64::MAX, Ordering::SeqCst);
        self.waker.register(context.waker());
        if stored == u64::MAX {
            None
        } else {
            Some(stored)
        }
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

    pub fn wake(self) {
        self.this_handle_woke
            .store(true, std::sync::atomic::Ordering::Release);
        self.waker.wake()
    }

    pub fn next_message_id(&self) -> u64 {
        self.message_id
    }
}
