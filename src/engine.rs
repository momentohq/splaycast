use futures::Stream;
use std::{
    collections::VecDeque,
    pin::{pin, Pin},
    sync::Arc,
    task::{Context, Poll},
};

use crate::{
    buffer_policy::{BufferInstruction, BufferPolicy},
    shared::{Shared, WakeHandle},
    SplaycastEntry,
};

/// An Engine is an api-less plugin to an event loop. It is an adapter between an
/// upstream Stream and downstream subscriber Streams.
///
/// Engine can do its work without blocking or synchronizing with the receivers.
/// This is true because Engine uses the raw `poll` affordance of Future, which
/// vends an &mut view of self.
pub struct Engine<Upstream, Item: Clone, Policy> {
    next_message_id: u64,
    upstream: Upstream,
    // TODO: buffer the buffers
    shared: Arc<Shared<Item>>,
    buffer_policy: Policy,
    parked_wakers: Vec<WakeHandle>,
    waking: Vec<WakeHandle>,
    wake_limit: usize,
}

impl<Upstream, Item, Policy> std::fmt::Debug for Engine<Upstream, Item, Policy>
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

impl<Upstream, Item, Policy> Engine<Upstream, Item, Policy>
where
    Upstream: futures::Stream<Item = Item> + Unpin,
    Item: Clone + Send,
    Policy: BufferPolicy<Item>,
{
    pub(crate) fn new(
        upstream: Upstream,
        shared: Arc<Shared<Item>>,
        buffer_policy: Policy,
    ) -> Self {
        Self {
            next_message_id: 1,
            upstream,
            shared,
            buffer_policy,
            parked_wakers: Default::default(),
            waking: Default::default(),
            wake_limit: 32,
        }
    }

    /// Set the maximum number of wakers to wake in a single poll cycle.
    /// Larger numbers are more efficient, but can lead to excessive poll times.
    pub fn set_wake_limit(&mut self, wake_limit: usize) {
        self.wake_limit = wake_limit.max(1)
    }

    fn absorb_upstream(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> (bool, Option<Poll<()>>) {
        let mut new_queue: Option<VecDeque<SplaycastEntry<Item>>> = None;

        let result = loop {
            let next = pin!(&mut self.upstream).poll_next(context);
            match next {
                Poll::Ready(state) => match state {
                    Some(item) => {
                        let new_queue = new_queue.get_or_insert_with(|| {
                            let shared_queue = self.shared.load_queue();
                            let mut new_queue = VecDeque::new();
                            new_queue.clone_from(shared_queue.as_ref());
                            new_queue
                        });
                        while BufferInstruction::Pop
                            == new_queue
                                .front()
                                .map(|buffer_tail| {
                                    self.buffer_policy.buffer_tail_policy(&buffer_tail.item)
                                })
                                .unwrap_or(BufferInstruction::Retain)
                        {
                            let mut oldest = new_queue
                                .pop_front()
                                .expect("front was checked above; this is removing the value");
                            self.buffer_policy.on_after_pop(&mut oldest.item);
                        }
                        let id = self.next_message_id;
                        self.next_message_id += 1;

                        let mut entry = SplaycastEntry { id, item };
                        log::trace!("new entry id {}", entry.id);
                        self.buffer_policy.on_before_send(&mut entry.item);

                        new_queue.push_back(entry);
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

        if let Some(new_queue) = new_queue {
            // TODO: buffer the buffers
            // This new queue process is too expensive per message, but sharing will require some clever
            // or optimistic arc swapping.
            let _to_buffer = self.shared.swap_queue(new_queue);
            (true, result)
        } else {
            (false, result)
        }
    }
}

/// Safety: I don't use unsafe for this type
impl<Upstream, Item, Policy> Unpin for Engine<Upstream, Item, Policy> where Item: Clone {}

impl<Upstream, Item, Policy> futures::Future for Engine<Upstream, Item, Policy>
where
    Upstream: futures::Stream<Item = Item> + Unpin,
    Item: Clone + Send,
    Policy: BufferPolicy<Item>,
{
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        log::trace!("poll: {self:?}");
        if self.shared.is_dead() {
            self.wake_everybody_because_i_am_dead();
            return Poll::Ready(());
        }

        self.shared.register_wake_interest(context); // In case we woke from a new waker, let's make sure it happens again

        let (dirty, early_out) = self.as_mut().absorb_upstream(context);
        if let Some(early_out) = early_out {
            log::trace!("upstream died - terminating the splaycast"); // this happens when the upstream is closed
            self.shared.set_dead();
            self.wake_everybody_because_i_am_dead();
            return early_out;
        }
        // Upstream is Pending here.

        if dirty {
            log::trace!("notifying parked: {}", self.parked_wakers.len());
            let Self {
                waking,
                parked_wakers,
                ..
            } = &mut *self;
            if waking.is_empty() {
                std::mem::swap(waking, parked_wakers);
            } else {
                waking.append(parked_wakers);
            }
        }
        if !self.waking.is_empty() {
            for _ in 0..self.wake_limit {
                if let Some(waker) = self.waking.pop() {
                    waker.wake();
                } else {
                    break;
                }
            }
            if !self.waking.is_empty() {
                context.waker().wake_by_ref();
            }
        }

        // Service downstreams
        for (serviced, waker) in self.shared.drain_wakelist().enumerate() {
            let tip = self.next_message_id - 1;
            if tip < waker.next_message_id() {
                log::trace!("tip at {tip}, parking at {}", waker.next_message_id());
                self.parked_wakers.push(waker);

                if self.wake_limit == serviced {
                    context.waker().wake_by_ref();
                    break;
                }
                continue; // this waker does not need to be woken. We parked it waiting new data
            }
            log::trace!("waking at {}", waker.next_message_id());
            waker.wake();

            if self.wake_limit == serviced {
                context.waker().wake_by_ref();
                break;
            }
        }

        // Awaiting an upstream message, for which we are already Pending, and we've woken what we need to
        log::trace!("parked pending");
        Poll::Pending
    }
}

impl<Upstream, Item: Clone, Policy> Engine<Upstream, Item, Policy> {
    fn wake_everybody_because_i_am_dead(&mut self) {
        log::trace!("is dead - waking everyone");
        for waker in std::mem::take(&mut self.parked_wakers) {
            waker.wake();
        }
        for waker in self.shared.drain_wakelist() {
            waker.wake();
        }
        log::trace!("all all wake handles have been notified. Completing the Engine task");
    }
}

impl<Upstream, Item: Clone, Policy> Drop for Engine<Upstream, Item, Policy> {
    fn drop(&mut self) {
        log::trace!("dropping splaycast Engine");
        self.shared.set_dead();
        self.wake_everybody_because_i_am_dead()
    }
}
