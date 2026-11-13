//! Move-only ownership, cancellation, and capability misuse tests.

use std::sync::Arc;

use chigiri_http::SubscriptionTransport;
use chigiri_runtime::SubscriptionOverflow;

use super::super::{
    SubscriptionOutbox, SubscriptionOutboxSignal, SubscriptionSendFrame, SubscriptionSendProgress,
};
use super::fixture::{EVENT_LIMIT, enqueue, outbox, settings};
use crate::subscription::delivery::EndpointByteOwner;

#[test]
fn frame_is_send_static_as_ref_and_not_clone() {
    assert_owner::<SubscriptionSendFrame>();
    assert_send_static::<SubscriptionOutbox>();
    assert_send_static::<SubscriptionSendProgress>();
    let _ = <SubscriptionSendFrame as AmbiguousIfClone<_>>::marker;
    let _ = <SubscriptionSendProgress as AmbiguousIfClone<_>>::marker;
}

#[test]
fn cancellation_releases_queue_but_stale_progress_cannot_mutate_state() {
    let settings = settings(2, 100, 100);
    let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
    let mut queued = outbox(
        &settings,
        owner.clone(),
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
    );
    enqueue(&mut queued, "queued", 0).unwrap();
    assert!(owner.used() > 0);
    queued.cancel();
    assert_eq!(owner.used(), 0);

    let mut sending = outbox(
        &settings,
        owner.clone(),
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
    );
    enqueue(&mut sending, "in-flight", 1).unwrap();
    let (frame, mut progress) = sending.begin_send(1).unwrap().unwrap();
    sending.cancel();
    assert!(owner.used() > 0);
    assert_eq!(
        progress.accept(1, 2).unwrap_err(),
        SubscriptionOutboxSignal::Cancelled
    );
    drop(frame);
    assert_eq!(owner.used(), 0);
}

#[test]
fn finishing_before_the_exact_frame_drops_fails_closed() {
    let settings = settings(2, 100, 100);
    let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
    let mut outbox = outbox(
        &settings,
        owner.clone(),
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
    );
    enqueue(&mut outbox, "event", 0).unwrap();
    let (frame, mut progress) = outbox.begin_send(0).unwrap().unwrap();
    assert!(progress.accept(frame.len(), 1).unwrap());
    assert_eq!(
        outbox.finish_send(progress, 1).unwrap_err(),
        SubscriptionOutboxSignal::Invariant
    );
    assert!(owner.used() > 0);
    drop(frame);
    assert_eq!(owner.used(), 0);
}

#[test]
fn foreign_finished_handle_is_rejected_by_exact_identity() {
    let settings = settings(2, 100, 100);
    let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
    let mut first = outbox(
        &settings,
        owner.clone(),
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
    );
    let mut second = outbox(
        &settings,
        owner,
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
    );
    let first_progress = completed_progress(&mut first, 0);
    let second_progress = completed_progress(&mut second, 1);
    assert_eq!(
        second.finish_send(first_progress, 2).unwrap_err(),
        SubscriptionOutboxSignal::Invariant
    );
    assert_eq!(
        second_progress.accepted_bytes().unwrap_err(),
        SubscriptionOutboxSignal::Invariant
    );
    first.cancel();
}

#[test]
fn backward_outbox_and_progress_time_become_sticky_terminal_failures() {
    let settings = settings(2, 100, 100);
    let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
    let mut outbox = outbox(
        &settings,
        owner,
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
    );
    enqueue(&mut outbox, "event", 10).unwrap();
    let (frame, mut progress) = outbox.begin_send(10).unwrap().unwrap();
    assert_eq!(
        progress.accept(0, 9).unwrap_err(),
        SubscriptionOutboxSignal::NonMonotonicTime
    );
    assert_eq!(
        outbox.next_deadline().unwrap_err(),
        SubscriptionOutboxSignal::NonMonotonicTime
    );
    drop(frame);
}

#[test]
fn one_clock_rejects_outbox_time_older_than_progress_time() {
    let settings = settings(2, 100, 100);
    let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
    let mut outbox = outbox(
        &settings,
        owner,
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
    );
    enqueue(&mut outbox, "event", 10).unwrap();
    let (frame, mut progress) = outbox.begin_send(10).unwrap().unwrap();
    assert!(!progress.accept(0, 12).unwrap());
    assert_eq!(
        outbox.on_progress(11).unwrap_err(),
        SubscriptionOutboxSignal::NonMonotonicTime
    );
    assert_eq!(
        progress.accepted_bytes().unwrap_err(),
        SubscriptionOutboxSignal::NonMonotonicTime
    );
    drop(frame);
}

#[test]
fn constructor_rejects_profile_and_operation_id_mismatch() {
    let settings = settings(2, 100, 100);
    let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
    assert_eq!(
        SubscriptionOutbox::new(
            &settings,
            owner.clone(),
            SubscriptionOverflow::Backpressure,
            SubscriptionTransport::WebSocket,
            None,
        )
        .err()
        .unwrap(),
        SubscriptionOutboxSignal::InvalidTransportArguments
    );
    assert_eq!(
        SubscriptionOutbox::new(
            &settings,
            owner,
            SubscriptionOverflow::Backpressure,
            SubscriptionTransport::Multipart,
            Some(Arc::from("unexpected")),
        )
        .err()
        .unwrap(),
        SubscriptionOutboxSignal::InvalidTransportArguments
    );
}

fn completed_progress(
    outbox: &mut SubscriptionOutbox,
    now_ns: u64,
) -> super::super::SubscriptionSendProgress {
    enqueue(outbox, "event", now_ns).unwrap();
    let (frame, mut progress) = outbox.begin_send(now_ns).unwrap().unwrap();
    assert!(progress.accept(frame.len(), now_ns).unwrap());
    drop(frame);
    progress
}

fn assert_owner<T: AsRef<[u8]> + Send + 'static>() {}

fn assert_send_static<T: Send + 'static>() {}

trait AmbiguousIfClone<Marker> {
    fn marker() {}
}

impl<T: ?Sized> AmbiguousIfClone<()> for T {}

struct Invalid;

impl<T: Clone> AmbiguousIfClone<Invalid> for T {}
