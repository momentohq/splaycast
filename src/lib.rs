//! A specialized version of Broadcast, for 1 Stream in -> many Streams out.
//!
//! # About
//! [`Splaycast`] is a Broadcast-adjacent tool for connecting streams. It has
//! only 1 way of publishing - it greedily consumes the upstream and tries to
//! send to all of the downstreams as quickly as possible.
//!
//! This project does not directly use `unsafe` code. Reputable dependencies
//! like arc-swap and crossbeam-queue are used here, which internally _do_ have
//! unsafe code.
//!
//! Direct dependencies are fairly trim, and while Splaycast is not tested on
//! other runtimes, it is expected that you can use something other than Tokio.
//!
//! # Details
//! Splaycast does not explicitly synchronize for publishing from upstream or
//! for vending to downstream receiver streams. A normal `channel()` usually
//! only has 2 main components - a sender and a receiver. `Splaycast` has 3:
//! 1. The [`splaycast::Engine`]. This drives the streams and notifies receivers. You spawn this on your runtime.
//! 2. The [`splaycast::Receiver`]. It's just a stream. You use it or compose it how you need to.
//! 3. The [`Splaycast`] itself. This is how you subscribe new receivers. It is not a sender, and you cannot send to it.
//!
//! ## Engine
//! The `splaycast::Engine` is a broadcast bridge. It is a raw `Future` which does
//! its work inside of `poll()`. By doing so, it has `&mut self`, permitting the
//! safe taking of liberties with data on the struct. There is no locking context
//! shared with `Receiver`s, no matter how brief.
//!
//! There are some easy optimizations available on the publish (upstream) end, but
//! Splaycast is intended to help most with high subscriber (downstream) counts so
//! it hasn't been a priority up to now.
//!
//! Receivers' wake handles are held in 1 of 2 places: The Wake Queue, or the Park List.
//!
//! The Wake Queue is a crossbeam queue, which efficiently handles multithreaded
//! registration of Wakers, e.g., on a multithreaded Tokio runtime. The wakers push
//! themselves into the wake queue, and the `Engine` pops them out when it runs.
//!
//! The Park List is where a wake handle goes when the upstream hasn't yielded the next
//! item that the wake handle's `Receiver` needs. It is a plain, owned Vec of wake handles.
//! Upon receipt of a new item from the upstream, the park list is unconditionally drained
//! and woken in one shot. There is no synchronization with `Receiver`s, so the expectation
//! is that those `Receiver`s may immediately begin consuming on another thread.
//! Being a plain Vec, the Park List will grow to match your subscriber count, and amortize
//! to no-allocation-cost over time.
//!
//! ## Receiver
//! The buffer (buffet-style, if you like) is shared with the `Receiver`s via
//! an ArcSwap. Each receiver is woken when there may be something new for them
//! to consume. The `Receiver` consumes from the then-current "buffet" Arc
//! item-by-item until it reaches the end of the buffer. Only then does it register
//! itself for wake with the Engine.
//!
//! ## Splaycast
//! You register new `Receiver`s through this component. It carries a reference to the Shared
//! structure that the 3 components of a Splaycast all share, which is all that is needed to
//! hook up a new Receiver.
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
///
/// You can wrap pretty much any futures::Stream. Notably, you need to
/// make sure your T is safely Cloneable. Plain owned data is fine, but
/// you need to be aware that the Receiver will call clone() on your data.
///
/// Calling clone on the Receiver helps to make sure multiple threads can
/// make progress at the same time, and consequentially it _may_ reduce
/// the value of doing an Arc<> wrap for things like an intermediate Tonic
/// broadcast stream.
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
