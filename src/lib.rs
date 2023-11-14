//! A specialized version of Broadcast, for 1 Stream in -> many Streams out.
//!
//! [`Splaycast`] is a Broadcast-adjacent tool for connecting streams. It has
//! only 1 way of publishing - it greedily consumes the upstream and tries to
//! send to all of the downstreams as quickly as possible.
//!
//! Splaycast does not explicitly synchronize for publishing from upstream or
//! for vending to downstream receiver streams. A normal `channel()` usually
//! only has 2 main components - a sender and a receiver. `Splaycast` has 3:
//! 1. The [`Splaycast`] itself. This is how you subscribe new receivers. It is not a sender, and you cannot send to this.
//! 2. The [`splaycast::Engine`]. This drives the streams and notifies receivers. You spawn this on your runtime.
//! 3. The [`splaycast::Receiver`]. It's just a stream.
//!
//! # Examples
//!
//! Some basic examples can be found under `src/benches`.
//!
//! # Feature Flags
//!

mod engine;
mod receiver;
mod shared;
mod splaycast;

pub enum SplaycastMessage<T> {
    /// Something from the upstream.
    Entry { item: T },
    /// From splaycast, this tells you how many messages you missed.
    /// Consume faster, publish slower, or possibly buffer more to reduce these!
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

pub use engine::Engine;
pub use receiver::Receiver;
pub use splaycast::Splaycast;

/// Wrap a stream with a Splaycast - a broadcast channel for streams.
///
/// This function returns you a tuple:
/// * An Engine you need to spawn on your async runtime.
/// * A Splaycast handle to which you may `subscribe()`.
pub fn wrap<T, Upstream>(
    upstream: Upstream,
    buffer_size: usize,
) -> (Engine<Upstream, T>, Splaycast<T>)
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
