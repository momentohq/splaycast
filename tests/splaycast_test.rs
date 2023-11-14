use std::{
    pin::pin,
    task::{Context, Poll},
};

use futures::{task::noop_waker_ref, Future, Stream};
use splaycast::{Engine, Message, Splaycast};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio_stream::{wrappers::UnboundedReceiverStream, StreamExt};

fn get_splaycast() -> (
    UnboundedSender<usize>,
    Splaycast<usize>,
    Engine<UnboundedReceiverStream<usize>, usize>,
) {
    let (publish_handle, upstream) = unbounded_channel::<usize>();
    let upstream = UnboundedReceiverStream::new(upstream); // Ideally you'd use an upstream from something like a tonic server streaming response
    let (engine, splaycast) = splaycast::wrap(upstream, 2);
    (publish_handle, splaycast, engine)
}

/// These tests are doing raw poll rather than await to make sure the futures are doing exactly what they are supposed to do, when they are supposed to do it.
fn poll<T, F: futures::Future<Output = T> + Unpin>(future: &mut F) -> Poll<T> {
    pin!(future).poll(&mut Context::from_waker(noop_waker_ref()))
}

fn poll_next<T, F: futures::Stream<Item = T> + Unpin>(stream: &mut F) -> Poll<Option<T>> {
    pin!(stream).poll_next(&mut Context::from_waker(noop_waker_ref()))
}

fn entry<T>(item: T) -> Option<Message<T>> {
    Some(Message::Entry { item })
}

fn lag<T>(count: usize) -> Option<Message<T>> {
    Some(Message::Lagged { count })
}

#[allow(clippy::expect_used)] // i mean, it's a test
#[test]
fn empty_snapshot_wake_list() {
    let (publish_handle, splaycast, mut engine) = get_splaycast();

    let mut subscriber = splaycast.subscribe();
    let mut next = pin!(subscriber.next());
    assert_eq!(
        Poll::Pending,
        poll(&mut next),
        "There isn't a next entry yet"
    );

    publish_handle.send(4).expect("unbound send");
    assert_eq!(Poll::Pending, poll(&mut engine)); // Drive the engine 1 step

    assert_eq!(
        Poll::Ready(entry(4_usize)),
        poll(&mut next),
        "Engine should have woken this subscriber from the wake list"
    );
}

#[allow(clippy::expect_used)] // i mean, it's a test
#[test]
fn empty_snapshot_park_list() {
    let (publish_handle, splaycast, mut engine) = get_splaycast();

    let mut subscriber = splaycast.subscribe();
    assert_eq!(Poll::Pending, poll(&mut engine)); // Drive the engine 1 step

    let mut next = pin!(subscriber.next());
    assert_eq!(
        Poll::Pending,
        poll(&mut next),
        "There isn't a next entry yet"
    );
    assert_eq!(
        Poll::Pending,
        poll(&mut engine),
        "move the subscriber to the park list"
    );

    publish_handle.send(4).expect("unbounded send");
    assert_eq!(Poll::Pending, poll(&mut engine)); // Drive the engine 1 step

    assert_eq!(
        Poll::Ready(entry(4_usize)),
        poll(&mut next),
        "Engine should have woken this subscriber from the park list"
    );
}

#[allow(clippy::expect_used)] // i mean, it's a test
#[test]
fn join_active_splaycast() {
    let (publish_handle, splaycast, mut engine) = get_splaycast();
    (0..100).for_each(|i| publish_handle.send(i).expect("unbounded send"));

    let mut subscriber = splaycast.subscribe();
    assert_eq!(
        Poll::Pending,
        poll(&mut engine),
        "accept 100 messages and 1 subscriber"
    );

    let mut next = pin!(subscriber.next());
    assert_eq!(
        Poll::Ready(entry(99)),
        poll(&mut next),
        "The queue length is 2, but we resume from the tip"
    );
    assert_eq!(
        Poll::Pending,
        poll(&mut engine),
        "move the subscriber to the park list"
    );

    publish_handle.send(4).expect("unbounded send");
    assert_eq!(Poll::Pending, poll(&mut engine)); // Drive the engine 1 step

    assert_eq!(
        Poll::Ready(entry(4_usize)),
        poll(&mut next),
        "Engine should have woken this subscriber from the park list"
    );
}

#[test]
fn subscriber_count() {
    let (_publish_handle, splaycast, mut _engine) = get_splaycast();
    let _subscribers: Vec<splaycast::Receiver<usize>> =
        (0..100).map(|_| splaycast.subscribe()).collect();

    assert_eq!(
        100,
        splaycast.subscriber_count(),
        "I subscribed 100 subscribers"
    );
}

#[allow(clippy::expect_used)] // i mean, it's a test
#[test]
fn splay_to_subscribers() {
    let (publish_handle, splaycast, mut engine) = get_splaycast();
    let mut subscribers: Vec<splaycast::Receiver<usize>> =
        (0..100).map(|_| splaycast.subscribe()).collect();
    for result in subscribers.iter_mut().map(poll_next) {
        assert_eq!(Poll::Pending, result, "everybody is pending");
    }
    publish_handle.send(4).expect("unbounded stream");
    publish_handle.send(2).expect("unbounded stream");
    for result in subscribers.iter_mut().map(poll_next) {
        assert_eq!(
            Poll::Pending,
            result,
            "upstream availability does not make the engine move forward. It only wakes it."
        );
    }

    assert_eq!(
        Poll::Pending,
        poll(&mut engine),
        "consume 2 messages and wake all the subscribers"
    );

    for result in subscribers.iter_mut().map(poll_next) {
        assert_eq!(
            Poll::Ready(entry(4)),
            result,
            "everybody sees messages in order, and 4 came first"
        );
    }
    for result in subscribers.iter_mut().map(poll_next) {
        assert_eq!(
            Poll::Ready(entry(2)),
            result,
            "everybody sees messages in order, and 2 came second"
        );
    }
    for result in subscribers.iter_mut().map(poll_next) {
        assert_eq!(
            Poll::Pending,
            result,
            "everybody consumed to the tip and are now registered for wake"
        );
    }

    publish_handle.send(6).expect("unbounded stream");
    assert_eq!(
        Poll::Pending,
        poll(&mut engine),
        "consume 1 message and wake all the subscribers"
    );
    for result in subscribers.iter_mut().map(poll_next) {
        assert_eq!(
            Poll::Ready(entry(6)),
            result,
            "everybody sees messages in order, and 6 comes last"
        );
    }
    for result in subscribers.iter_mut().map(poll_next) {
        assert_eq!(
            Poll::Pending,
            result,
            "everybody consumed to the tip and are now registered for wake"
        );
    }
}

#[allow(clippy::expect_used)] // i mean, it's a test
#[test]
fn slow_subscriber() {
    let (publish_handle, splaycast, mut engine) = get_splaycast();
    let mut fast_subscriber: splaycast::Receiver<usize> = splaycast.subscribe();
    let mut slow_subscriber: splaycast::Receiver<usize> = splaycast.subscribe();
    assert_eq!(
        Poll::Pending,
        poll_next(&mut slow_subscriber),
        "slowly consumes, but needs to be initialized"
    );

    for i in 0..10 {
        publish_handle.send(i).expect("unbounded stream");
        assert_eq!(
            Poll::Pending,
            poll(&mut engine),
            "consume 1 message and wake all the subscribers"
        );
        assert_eq!(
            Poll::Ready(entry(i)),
            poll_next(&mut fast_subscriber),
            "quickly consumes"
        );
    }

    assert_eq!(Poll::Ready(lag(9)), poll_next(&mut slow_subscriber), "Yes, we published 10 to a queue of 2. When we lag we move to the tip - this shouldn't be 8!");
    assert_eq!(
        Poll::Ready(entry(9)),
        poll_next(&mut slow_subscriber),
        "Skipped to the front of the queue, but not off the end"
    );
    assert_eq!(
        Poll::Pending,
        poll_next(&mut slow_subscriber),
        "Should be all caught up with the fast subscriber"
    );
    assert_eq!(
        Poll::Pending,
        poll_next(&mut fast_subscriber),
        "Should be all caught up with the slow subscriber"
    );
}
