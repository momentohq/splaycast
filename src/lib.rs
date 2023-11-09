//! A specialized version of Broadcast, for 1 Stream in -> many Streams out.
//!
//! [`Splaycast`] is a Broadcast-adjacent tool for connecting streams. It has
//! only 1 way of publishing - it greedily consumes the upstream and tries to
//! send to all of the downstreams as quickly as possible.
//!
//! Splaycast does not explicitly synchronize for publishing from upstream or
//! for vending to downstream receiver streams. A normal `channel()` usually
//! only has 2 main components - a sender and a receiver. `Splaycast` has 3:
//! 1. The [`Splaycast`] itself. This is how you subscribe new receivers.
//! 2. The [`splaycast::Engine`]. This drives the streams and notifies receivers. You spawn this on your runtime.
//! 3. The [`splaycast::Receiver`]. It's just a stream.
//!
//! # Examples
//!
//! Some basic examples can be found under `src/benches`.
//!
//! # Feature Flags
//!

mod shared;
mod splaycast;
mod engine;
mod receiver;

pub enum SplaycastMessage<T> {
    Entry { item: T },
    Lagged { count: usize },
}

impl<T> std::fmt::Debug for SplaycastMessage<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Entry { item: _ } => f.debug_struct("Entry").finish_non_exhaustive(),
            Self::Lagged { count } => f.debug_struct("Lagged").field("count", count).finish(),
        }
    }
}

pub use splaycast::Splaycast;
pub use engine::Engine;
pub use receiver::Receiver;

pub fn wrap<T, Upstream>(upstream: Upstream, buffer_size: usize) -> (Engine<Upstream, T>, Splaycast<T>)
where
    T: Clone + Send + Unpin,
    Upstream: futures::Stream<Item = T> + Unpin,
{
    Splaycast::new(upstream, buffer_size)
}

#[derive(Clone, Debug)]
pub(crate) struct SplaycastEntry<T> {
    pub id: u64,
    pub item: T,
}

impl<T> SplaycastEntry<T> {
    pub fn id(&self) -> u64 {
        self.id
    }
}
