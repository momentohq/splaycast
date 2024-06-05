use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use crossbeam_queue::ArrayQueue;
use futures::{task::AtomicWaker, Stream};

/// A single-producer sender, for a splaycast.
///
/// If you're producing items for a splaycast in a way other than streaming, you
/// may use a Sender as an adapter.
///
/// This will introduce some looseness in your
/// splaycast's size limit. Splaycast drains the sender as quickly as possible:
/// As long as you're not sustaining a higher send rate than the splaycast
/// engine can drain, you should see memory usage track pretty closely to your
/// splaycast buffer size, and not much worse than 2*buffer size worst case.
pub struct Sender<T> {
    queue: Arc<ArrayQueue<T>>,
    waker: Arc<AtomicWaker>,
}

impl<T> Sender<T> {
    /// Send a value. If the send buffer is full, you'll get your value back as the Err value.
    /// If you get an Err often, you probvably need a larger splaycast buffer or you need to
    /// make the splaycast Engine run more often (e.g., by adding more threads to your runtime
    /// or other task throughput enhancements)
    pub fn send(&self, item: T) -> Result<(), T> {
        match self.queue.push(item) {
            Ok(_) => {
                self.waker.wake();
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    pub(crate) fn new(buffer_size: usize) -> (Self, SenderStream<T>) {
        let queue = Arc::new(ArrayQueue::new(buffer_size));
        let waker = Arc::new(AtomicWaker::new());
        (
            Self {
                queue: queue.clone(),
                waker: waker.clone(),
            },
            SenderStream { queue, waker },
        )
    }
}

pub struct SenderStream<T> {
    queue: Arc<ArrayQueue<T>>,
    waker: Arc<AtomicWaker>,
}

impl<T> Stream for SenderStream<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.waker.register(context.waker());
        match self.queue.pop() {
            Some(more) => Poll::Ready(Some(more)),
            None => Poll::Pending, // already waiting for the waker, possibly even already woken
        }
    }
}
