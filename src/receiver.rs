use std::{
    cmp::min,
    collections::VecDeque,
    pin::Pin,
    sync::{atomic::AtomicBool, Arc},
    task::{Context, Poll},
};

use crate::{
    shared::{Shared, WakeHandle},
    SplaycastEntry, SplaycastMessage,
};

pub struct Receiver<Item>
where
    Item: Clone,
{
    shared: Arc<Shared<Item>>,
    next_message_id: u64,
    dirty: Arc<AtomicBool>,
}

impl<Item> std::fmt::Debug for Receiver<Item>
where
    Item: Clone,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SplaycastReceiver")
            .field("next", &self.next_message_id)
            .field(
                "dirty",
                &self.dirty.load(std::sync::atomic::Ordering::Relaxed),
            )
            .finish()
    }
}

impl<Item> Receiver<Item>
where
    Item: Clone,
{
    pub(crate) fn new(shared: Arc<Shared<Item>>) -> Self {
        shared.increment_subscriber_count();
        Self {
            shared,
            next_message_id: 0,
            dirty: Arc::new(AtomicBool::new(true)),
        }
    }

    fn mark_clean_and_register_for_wake(&mut self, context: &mut Context<'_>) {
        self.dirty
            .store(false, std::sync::atomic::Ordering::Release);
        self.shared.register_waker(WakeHandle::new(
            self.next_message_id,
            context.waker().clone(),
            self.dirty.clone(),
        ));
    }
}

impl<Item> Drop for Receiver<Item>
where
    Item: Clone,
{
    fn drop(&mut self) {
        self.shared.decrement_subscriber_count();
    }
}

impl<Item> futures::Stream for Receiver<Item>
where
    Item: Clone,
{
    type Item = SplaycastMessage<Item>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        log::trace!("poll {self:?}");
        if !self.dirty.load(std::sync::atomic::Ordering::Acquire) {
            log::trace!("pending clean");
            return Poll::Pending; // We haven't gotten anything new yet.
        }
        let shared_queue_snapshot = self.shared.load_queue();
        if shared_queue_snapshot.is_empty() {
            log::trace!("pending clean - empty snapshot");
            self.mark_clean_and_register_for_wake(context);
            return Poll::Pending; // We're registered for wake on delivery of new items at the next message id.
        }

        if self.next_message_id == 0 {
            self.next_message_id = shared_queue_snapshot
                .back()
                .expect("I checked and this was not empty")
                .id
        }

        let index = match find(self.next_message_id, &shared_queue_snapshot) {
            Ok(found) => found,
            Err(missing_at) => {
                if missing_at == 0 {
                    // We fell off the buffer.
                    let tip_id = shared_queue_snapshot
                        .back()
                        .expect("I checked and this was not empty")
                        .id;
                    let lag = SplaycastMessage::Lagged {
                        count: (tip_id - self.next_message_id) as usize,
                    };
                    self.next_message_id = tip_id;
                    log::trace!("ready lag - {lag:?}");
                    return Poll::Ready(Some(lag));
                } else if missing_at == shared_queue_snapshot.len() {
                    // We're caught up.
                    log::trace!("pending clean - caught up");
                    self.mark_clean_and_register_for_wake(context);
                    return Poll::Pending; // We're registered for wake on delivery of new items at the next message id.
                } else {
                    log::error!("ids must be sequential");
                    return Poll::Ready(None);
                }
            }
        };

        let message_id = shared_queue_snapshot[index].id;
        log::trace!("ready at {message_id}");
        self.next_message_id = message_id + 1;
        Poll::Ready(Some(SplaycastMessage::Entry {
            item: shared_queue_snapshot[index].item.clone(),
        }))
    }
}

#[inline]
fn find<Item>(id: u64, buffer: &VecDeque<SplaycastEntry<Item>>) -> Result<usize, usize> {
    if buffer.is_empty() {
        return Err(0);
    }
    // Try to optimize the case where the subscriber is close to the end
    for i in 0..min(4, buffer.len()) {
        let index = buffer.len() - 1 - i;
        if buffer[index].id < id {
            // the index is past this. This is just an early out for when a subscriber is caught up.
            return Err(index + 1);
        }
        if buffer[index].id == id {
            return Ok(index);
        }
    }
    buffer.binary_search_by_key(&id, SplaycastEntry::id)
}
