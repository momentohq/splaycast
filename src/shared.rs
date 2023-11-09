use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicUsize},
        Arc, Mutex,
    },
};

use arc_swap::ArcSwap;

use crate::SplaycastEntry;

pub struct Shared<Item>
where
    Item: Clone,
{
    subscriber_count: AtomicUsize,
    wakers: Arc<Mutex<VecDeque<WakeHandle>>>,
    queue: Arc<ArcSwap<VecDeque<SplaycastEntry<Item>>>>,
}

impl<Item> std::fmt::Debug for Shared<Item>
where
    Item: Clone,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let m = self.wakers.lock().expect("local mutex");
        f.debug_struct("Shared")
            .field("subscriber_count", &self.subscriber_count)
            .field("wakers_count", &m.len())
            .field(
                "wakers",
                &m.iter()
                    .map(WakeHandle::next_message_id)
                    .map(|n| n.to_string())
                    .collect::<Vec<String>>()
                    .join(","),
            )
            // .field("queue", &self.queue)
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

    pub fn swap_wakers(&self, buffer: &mut VecDeque<WakeHandle>) {
        let mut wakers = self.wakers.lock().expect("local mutex");
        log::trace!("swap wakers length {} -> {}", wakers.len(), buffer.len());
        std::mem::swap(&mut *wakers, buffer)
    }

    pub fn register_waker(&self, handle: WakeHandle) {
        log::trace!("register waker at {}", handle.message_id);
        self.wakers.lock().expect("local mutex").push_back(handle)
    }

    pub fn register_waker_batch(&self, batch: impl IntoIterator<Item = WakeHandle>) {
        self.wakers.lock().expect("local mutex").extend(batch)
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
