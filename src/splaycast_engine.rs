use futures::Stream;
use std::{
    collections::VecDeque,
    pin::{pin, Pin},
    sync::Arc,
    task::{Context, Poll},
};

use crate::{
    shared::{Shared, WakeHandle},
    SplaycastEntry,
};

pub struct SplaycastEngine<Upstream, Item>
where
    Item: Clone,
{
    next_message_id: u64,
    upstream: Upstream,
    // TODO: buffer the buffers
    shared: Arc<Shared<Item>>,
    wakers_in_progress: VecDeque<WakeHandle>,
}

impl<Upstream, Item> std::fmt::Debug for SplaycastEngine<Upstream, Item>
where
    Item: Clone,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SplaycastEngine")
            .field("next_message_id", &self.next_message_id)
            .field("shared", &self.shared)
            .finish()
    }
}

/// A SplaycastEngine is an api-less plugin to an event loop. It is an adapter between an
/// upstream Stream and a downstream Stream.
///
/// SplaycastEngine can do its work without blocking or synchronizing with the receivers.
/// This is true because SplaycastEngine uses the raw `poll` affordance of Future, which
/// vends an &mut view of self. There is a low frequency lock that is acquired to grab the
/// list of wakers waiting for a new message, but that's the extent of the locking.
impl<Upstream, Item> SplaycastEngine<Upstream, Item>
where
    Upstream: futures::Stream<Item = Item> + Unpin,
    Item: Clone + Send,
{
    pub(crate) fn new(upstream: Upstream, shared: Arc<Shared<Item>>) -> Self {
        Self {
            next_message_id: 0,
            upstream,
            shared,
            wakers_in_progress: Default::default(),
        }
    }

    fn absorb_upstream(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> (bool, Option<Poll<()>>) {
        let mut new_queue = {
            let shared_queue = self.shared.load_queue();
            let mut new_queue = VecDeque::with_capacity(shared_queue.capacity());
            new_queue.clone_from(shared_queue.as_ref());
            new_queue
        };
        let start_message = self.next_message_id;

        let result = loop {
            match pin!(&mut self.upstream).poll_next(context) {
                Poll::Ready(state) => match state {
                    Some(item) => {
                        if new_queue.capacity() == new_queue.len() {
                            new_queue.pop_front();
                        }
                        let id = self.next_message_id;
                        self.next_message_id += 1;
                        new_queue.push_back(SplaycastEntry { id, item });
                    }
                    None => {
                        log::debug!("upstream closed");
                        break Some(Poll::Ready(()));
                    }
                },
                Poll::Pending => {
                    log::trace!("nothing more upstream. Let's continue to send to downstreams");
                    break None;
                }
            }
        };

        if self.next_message_id != start_message {
            // todo: buffer the buffers
            let _to_buffer = self.shared.swap_queue(new_queue);
            (true, result)
        } else {
            (false, result)
        }
    }
}

/// Safety: I don't use unsafe for this type
impl<Upstream, Item> Unpin for SplaycastEngine<Upstream, Item> where Item: Clone {}

impl<Upstream, Item> futures::Future for SplaycastEngine<Upstream, Item>
where
    Upstream: futures::Stream<Item = Item> + Unpin,
    Item: Clone + Send,
{
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        log::trace!("poll: {self:?}");
        let (dirty, early_out) = self.as_mut().absorb_upstream(context);
        if let Some(early_out) = early_out {
            log::trace!("early out");
            return early_out;
        }
        // Upstream is Pending here.

        if !dirty {
            log::trace!("clean pending");
            // I woke everybody and nothing new happened yet. Let's wait for upstream messages.
            return Poll::Pending;
        }

        // Service downstreams
        let shared = self.shared.clone(); // to work around borrow checker shenanigans
        shared.swap_wakers(&mut self.wakers_in_progress);

        if self.wakers_in_progress.is_empty() {
            log::trace!("nobody waiting pending");
            // nobody is waiting for this topic.
            return Poll::Pending;
        }

        let next_message_id = self.next_message_id;

        let wakers_in_progress_count = self.wakers_in_progress.len();
        for _ in 0..wakers_in_progress_count {
            let waker = self.wakers_in_progress.pop_front().expect("I checked this boundary");
            if next_message_id < waker.next_message_id() {
                log::trace!("requeueing at {}", waker.next_message_id());
                self.wakers_in_progress.push_back(waker);
                continue; // this waker does not need to be woken
            }
            log::trace!("waking at {}", waker.next_message_id());
            waker.wake()
        }
        if !self.wakers_in_progress.is_empty() {
            shared.register_waker_batch(self.wakers_in_progress.drain(..));
        }

        // Awaiting an upstream message, for which we are already Pending, and we've woken what we need to
        log::trace!("notified batch pending");
        Poll::Pending
    }
}
