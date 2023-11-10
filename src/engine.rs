use futures::Stream;
use std::{
    collections::VecDeque,
    pin::{pin, Pin},
    sync::Arc,
    task::{Context, Poll},
};

use crate::{shared::Shared, SplaycastEntry};

pub struct Engine<Upstream, Item>
where
    Item: Clone,
{
    next_message_id: u64,
    upstream: Upstream,
    // TODO: buffer the buffers
    shared: Arc<Shared<Item>>,
}

impl<Upstream, Item> std::fmt::Debug for Engine<Upstream, Item>
where
    Item: Clone,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("next_message_id", &self.next_message_id)
            .field("shared", &self.shared)
            .finish()
    }
}

/// Am Engine is an api-less plugin to an event loop. It is an adapter between an
/// upstream Stream and a downstream Stream.
///
/// Engine can do its work without blocking or synchronizing with the receivers.
/// This is true because Engine uses the raw `poll` affordance of Future, which
/// vends an &mut view of self. There is a low frequency lock that is acquired to grab the
/// list of wakers waiting for a new message, but that's the extent of the locking.
impl<Upstream, Item> Engine<Upstream, Item>
where
    Upstream: futures::Stream<Item = Item> + Unpin,
    Item: Clone + Send,
{
    pub(crate) fn new(upstream: Upstream, shared: Arc<Shared<Item>>) -> Self {
        Self {
            next_message_id: 0,
            upstream,
            shared,
        }
    }

    fn absorb_upstream(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> (bool, Option<Poll<()>>) {
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
impl<Upstream, Item> Unpin for Engine<Upstream, Item> where Item: Clone {}

impl<Upstream, Item> futures::Future for Engine<Upstream, Item>
where
    Upstream: futures::Stream<Item = Item> + Unpin,
    Item: Clone + Send,
{
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        log::trace!("poll: {self:?}");
        let (dirty, early_out) = self.as_mut().absorb_upstream(context);
        if let Some(early_out) = early_out {
            log::trace!("early out"); // this happens when the upstream is closed
            return early_out;
        }
        let next_message_id = self.next_message_id;
        // Upstream is Pending here.

        let wake_interest = self.shared.load_and_reset_wake_interest(context);
        let some_wakers_need_serviced = match wake_interest {
            Some(wake_interest) => wake_interest < next_message_id,
            None => false,
        };

        if !(dirty || some_wakers_need_serviced) {
            log::trace!("clean pending");
            // I woke everybody and nothing new happened yet. Let's wait for upstream messages.
            return Poll::Pending;
        }

        // Service downstreams
        for waker in self.shared.iterate() {
            if next_message_id < waker.next_message_id() {
                log::trace!("requeueing at {}", waker.next_message_id());
                self.shared.register_waker(waker);
                continue; // this waker does not need to be woken
            }
            log::trace!("waking at {}", waker.next_message_id());
            waker.wake()
        }

        // Awaiting an upstream message, for which we are already Pending, and we've woken what we need to
        log::trace!("notified batch pending");
        Poll::Pending
    }
}
