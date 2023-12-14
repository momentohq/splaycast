//! A specialized version of Broadcast, for 1 Stream in -> many Streams out.
//!
//! # About
//! [`Splaycast`] is a Broadcast-adjacent tool for connecting streams. It
//! greedily consumes the upstream and tries to send to all of the
//! downstreams as quickly as possible.
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
//! If you drop [1] the Upstream Stream, [2] the Splaycast, or [3] the Engine, the
//! splaycast is terminated and everything is dropped. Your Receivers *_will receive
//! prompt notification_* of the termination of any critical upstream resource.
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
//! The most basic usage of splaycast which approximates a normal broadcast channel:
//! ```
//! # use futures::StreamExt;
//! # use splaycast::Message;
//! # tokio_test::block_on(async {
//! let (mut sender, engine, splaycast) = splaycast::channel(128);
//! tokio::spawn(engine);
//!
//! let mut receiver = splaycast.subscribe();
//! sender.send("hello");
//!
//! let hello = receiver.next().await;
//! assert_eq!(Some(Message::Entry { item: "hello" }), hello);
//! # })
//! ```
//!
//! Some basic examples can be found under `src/benches`.
//!
//! # Feature Flags
//!

mod engine;
mod receiver;
mod sender;
mod shared;
mod splaycast;

/// Messages on a Splaycast Receiver are either an Entry or a Lagged. If you
/// lag, you'll get a count of how many messages were skipped, and then you'll
/// resume Entries from that point on.
#[derive(Debug, PartialEq)]
pub enum Message<T> {
    /// The item is cloned from the upstream stream.
    Entry { item: T },
    /// From splaycast, this tells you how many messages you missed.
    /// Consume faster, publish slower, or possibly buffer more to reduce these!
    Lagged { count: usize },
}

pub use engine::Engine;
pub use receiver::Receiver;
pub use sender::{Sender, SenderStream};
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

/// Get an spmc channel to splay out to streaming receivers.
///
/// A channel has send(item), while a wrap(upstream)'d splaycast has no
/// adapters before directly consuming a stream.
///
/// If you have a stream you want to duplicate, wrap() it. If you have
/// a collection or some computed value that you want to duplicate, use
/// a channel().
///
/// This is spmc to mirror wrap()'s Stream paradigm, which is also spmc.
/// ```
/// # use futures::StreamExt;
/// # use splaycast::Message;
/// # tokio_test::block_on(async {
/// let (mut sender, engine, splaycast) = splaycast::channel(128);
/// tokio::spawn(engine);
///
/// let mut receiver = splaycast.subscribe();
/// sender.send("hello");
///
/// let hello = receiver.next().await;
/// assert_eq!(Some(Message::Entry { item: "hello" }), hello);
/// # })
/// ```
pub fn channel<T>(buffer_size: usize) -> (Sender<T>, Engine<SenderStream<T>, T>, Splaycast<T>)
where
    T: Clone + Send + Unpin,
{
    let (sender, stream) = Sender::new(buffer_size);
    let (engine, splaycast) = Splaycast::new(stream, buffer_size);
    (sender, engine, splaycast)
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
