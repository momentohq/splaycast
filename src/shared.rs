use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
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
    wakers: Arc<Mutex<Vec<WakeHandle>>>,
    queue: Arc<ArcSwap<VecDeque<SplaycastEntry<Item>>>>,
    waker: AtomicWaker,
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
        }
    }

    #[inline]
    pub fn subscriber_count(&self) -> usize {
        self.subscriber_count
            .load(std::sync::atomic::Ordering::Acquire)
    }

    #[inline]
    pub fn increment_subscriber_count(&self) -> usize {
        let count = self
            .subscriber_count
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
            + 1;
        log::trace!("incrementing subscriber count to {count}");
        count
    }

    #[inline]
    pub fn decrement_subscriber_count(&self) -> usize {
        let count = self
            .subscriber_count
            .fetch_sub(1, std::sync::atomic::Ordering::AcqRel)
            - 1;
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
        let message_id = handle.message_id;
        log::trace!("register waker at {message_id}");
        self.wakers.lock().expect("local mutex").push(handle);
        self.waker.wake()
    }

    #[inline]
    pub fn register_wake_interest(&self, context: &mut Context) {
        self.waker.register(context.waker());
    }

    #[inline]
    pub fn swap_wakelist(&self, wakelist: Vec<WakeHandle>) -> Vec<WakeHandle> {
        std::mem::replace(&mut *self.wakers.lock().expect("local mutex"), wakelist)
    }
}

struct WakeIterator<T> where T: Clone {
    shared: Arc<Shared<T>>,
    i: usize,
}
impl<T: Clone> Iterator for WakeIterator<T> {
    type Item = WakeHandle;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.shared.wakers.lock().expect("local mutex").pop()
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
        self.this_handle_woke
            .store(true, Ordering::Release);
        self.waker.wake()
    }

    #[inline]
    pub fn next_message_id(&self) -> u64 {
        self.message_id
    }
}
