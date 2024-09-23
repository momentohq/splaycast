use std::{
    pin::pin,
    task::{Context, Poll},
};

use futures::{task::noop_waker_ref, Future, Stream};
use splaycast::{buffer_policy::BufferPolicy, Engine, Message, Splaycast};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio_stream::{wrappers::UnboundedReceiverStream, StreamExt};

fn get_splaycast() -> (
    UnboundedSender<usize>,
    Splaycast<usize>,
    Engine<UnboundedReceiverStream<usize>, usize, impl BufferPolicy<usize>>,
) {
    get_splaycast_with_buffer(2)
}

fn get_splaycast_with_buffer(
    length: usize,
) -> (
    UnboundedSender<usize>,
    Splaycast<usize>,
    Engine<UnboundedReceiverStream<usize>, usize, impl BufferPolicy<usize>>,
) {
    let (publish_handle, upstream) = unbounded_channel::<usize>();
    let upstream = UnboundedReceiverStream::new(upstream); // Ideally you'd use an upstream from something like a tonic server streaming response
    let (engine, splaycast) = splaycast::wrap(upstream, length);
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
#[test_log::test]
fn join_active_splaycast() {
    let (publish_handle, splaycast, mut engine) = get_splaycast_with_buffer(3);
    (0..100).for_each(|i| publish_handle.send(i).expect("unbounded send"));

    let mut subscriber = splaycast.subscribe();
    assert_eq!(
        Poll::Pending,
        poll(&mut engine),
        "accept 100 messages and 1 subscriber"
    );

    assert_eq!(
        Poll::Ready(lag(97)),
        poll(&mut pin!(subscriber.next())),
        "The queue length is 3, and the subscriber joined at 1."
    );
    assert_eq!(
        Poll::Ready(entry(97)),
        poll(&mut pin!(subscriber.next())),
        "The queue length is 3, and the subscriber joined at 1 - 97 is the front of the buffer"
    );
    assert_eq!(
        Poll::Pending,
        poll(&mut engine),
        "move the subscriber to the park list"
    );

    publish_handle.send(4).expect("unbounded send");
    assert_eq!(Poll::Pending, poll(&mut engine)); // Drive the engine 1 step

    assert_eq!(
        Poll::Ready(entry(98_usize)),
        poll(&mut pin!(subscriber.next())),
        "Engine accepted 4, and 97 should be dropped. This subscriber is now at the old end of the buffer"
    );

    assert_eq!(
        Poll::Ready(entry(99_usize)),
        poll(&mut pin!(subscriber.next())),
        "This subscriber is now in the middle of the buffer"
    );

    assert_eq!(
        Poll::Ready(entry(4_usize)),
        poll(&mut pin!(subscriber.next())),
        "Subscriber should be caught up"
    );

    assert_eq!(
        Poll::Pending,
        poll(&mut pin!(subscriber.next())),
        "Subscriber should be caught up"
    );

    let mut tail_subscriber = splaycast.subscribe_at_tail();
    assert_eq!(
        Poll::Ready(entry(99_usize)),
        poll(&mut pin!(tail_subscriber.next())),
        "Tail subscriber should receive old messages. I'm starting 1 past the oldest in an attempt to help win more join races without lags"
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
        "slowly consumes, has nothing at the start"
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

    assert_eq!(
        Poll::Ready(lag(8)),
        poll_next(&mut slow_subscriber),
        "Yes, we published 10 to a queue of 2.
        When we lag we move to the start of the buffer. This is done to try to minimize lag size.
        If your subscriber is always slow, this will make more lag messages than if it were to reconnect at the tip."
    );
    assert_eq!(
        Poll::Ready(entry(8)),
        poll_next(&mut slow_subscriber),
        "Skipped to the start of the queue"
    );
    assert_eq!(
        Poll::Ready(entry(9)),
        poll_next(&mut slow_subscriber),
        "can consume to the front of the queue"
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

#[test]
fn drop_splaycast() {
    let (_publish_handle, splaycast, mut engine) = get_splaycast();
    let mut parked_subscriber: splaycast::Receiver<usize> = splaycast.subscribe();
    assert_eq!(Poll::Pending, poll_next(&mut parked_subscriber),);
    assert_eq!(
        Poll::Pending,
        poll(&mut engine),
        "move subscriber to park list"
    );

    let mut wake_queue_subscriber: splaycast::Receiver<usize> = splaycast.subscribe();
    assert_eq!(
        Poll::Pending,
        poll_next(&mut wake_queue_subscriber),
        "this subscriber is only in the wake queue"
    );

    // Dropping the Splaycast kills the splaycast.
    drop(splaycast);

    assert_eq!(
        Poll::Ready(()),
        poll(&mut engine),
        "Engine terminates promptly upon being set dead"
    );

    assert_eq!(
        Poll::Ready(None),
        poll_next(&mut parked_subscriber),
        "subscriber promptly receives an end-of-stream"
    );
    assert_eq!(
        Poll::Ready(None),
        poll_next(&mut wake_queue_subscriber),
        "subscriber promptly receives an end-of-stream"
    );
}

#[test]
fn drop_engine() {
    let (_publish_handle, splaycast, mut engine) = get_splaycast();
    let mut parked_subscriber: splaycast::Receiver<usize> = splaycast.subscribe();
    assert_eq!(Poll::Pending, poll_next(&mut parked_subscriber),);
    assert_eq!(
        Poll::Pending,
        poll(&mut engine),
        "move subscriber to park list"
    );

    let mut wake_queue_subscriber: splaycast::Receiver<usize> = splaycast.subscribe();
    assert_eq!(
        Poll::Pending,
        poll_next(&mut wake_queue_subscriber),
        "this subscriber is only in the wake queue"
    );

    // Dropping the splaycast Engine kills the splaycast. Probably dropping Engine is a mistake, but it should still not leak subscribers!!!
    drop(engine);

    assert_eq!(
        Poll::Ready(None),
        poll_next(&mut parked_subscriber),
        "subscriber promptly receives an end-of-stream"
    );
    assert_eq!(
        Poll::Ready(None),
        poll_next(&mut wake_queue_subscriber),
        "subscriber promptly receives an end-of-stream"
    );
}

#[test]
fn drop_upstream() {
    let (publish_handle, splaycast, mut engine) = get_splaycast();
    let mut parked_subscriber: splaycast::Receiver<usize> = splaycast.subscribe();
    assert_eq!(Poll::Pending, poll_next(&mut parked_subscriber),);
    assert_eq!(
        Poll::Pending,
        poll(&mut engine),
        "move subscriber to park list"
    );

    let mut wake_queue_subscriber: splaycast::Receiver<usize> = splaycast.subscribe();
    assert_eq!(
        Poll::Pending,
        poll_next(&mut wake_queue_subscriber),
        "this subscriber is only in the wake queue"
    );

    // Dropping the publish handle for a typical Stream implementation should wake downstreams.
    // When Engine is awoken with a dead upstream, it kills the splaycast. The subscribers should all be promptly notified and all resources released.
    drop(publish_handle);

    assert_eq!(
        Poll::Ready(()),
        poll(&mut engine),
        "Engine sees the upstream is dead and wakes subscribers before releasing itself"
    );

    assert_eq!(
        Poll::Ready(None),
        poll_next(&mut parked_subscriber),
        "subscriber promptly receives an end-of-stream"
    );
    assert_eq!(
        Poll::Ready(None),
        poll_next(&mut wake_queue_subscriber),
        "subscriber promptly receives an end-of-stream"
    );
}

#[allow(clippy::expect_used)] // i mean, it's a test
#[test]
fn drop_downstreams() {
    let (publish_handle, splaycast, mut engine) = get_splaycast();
    let mut parked_subscriber: splaycast::Receiver<usize> = splaycast.subscribe();
    assert_eq!(Poll::Pending, poll_next(&mut parked_subscriber),);
    assert_eq!(
        Poll::Pending,
        poll(&mut engine),
        "move subscriber to park list"
    );

    let mut wake_queue_subscriber: splaycast::Receiver<usize> = splaycast.subscribe();
    assert_eq!(
        Poll::Pending,
        poll_next(&mut wake_queue_subscriber),
        "this subscriber is only in the wake queue"
    );

    // Dropping the subscribers does not kill the splaycast, and it can still continue to work
    drop(parked_subscriber);
    drop(wake_queue_subscriber);

    publish_handle.send(4).expect("unbounded send");
    assert_eq!(
        Poll::Pending,
        poll(&mut engine),
        "Engine receives a message and goes to wake the subscribers who no longer exist"
    );
    assert_eq!(0, splaycast.subscriber_count());
    assert_eq!(
        Poll::Pending,
        poll(&mut engine),
        "Engine is still happily pending"
    );
}
