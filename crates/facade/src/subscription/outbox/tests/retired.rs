//! A selected source failure must not erase a prior event's progress deadline.

use std::time::Duration;

use chigiri_http::SubscriptionTransport;
use chigiri_runtime::SubscriptionOverflow;

use super::super::SubscriptionOutboxSignal;
use super::fixture::{EVENT_LIMIT, enqueue, outbox, settings};
use crate::subscription::delivery::EndpointByteOwner;

#[test]
fn retiring_source_preserves_prior_progress_and_its_original_expiry() {
    let settings = settings(2, 100, 10);
    let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
    let mut outbox = outbox(
        &settings,
        owner.clone(),
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
    );
    enqueue(&mut outbox, "event", 0).unwrap();
    let (frame, mut progress) = outbox.begin_send(0).unwrap().unwrap();
    assert!(!progress.accept(1, 5).unwrap());
    outbox.cancel();
    let retired = progress.retire(Duration::from_nanos(10));
    assert_eq!(retired.next_deadline().unwrap(), Some(15));
    retired.on_timer(14).unwrap();
    assert_eq!(
        retired.on_timer(15),
        Err(SubscriptionOutboxSignal::SendStalled)
    );
    assert_eq!(owner.used(), frame.len());
    drop(frame);
    assert_eq!(owner.used(), 0);
}

#[test]
fn whole_acceptance_after_source_failure_disables_stall_but_keeps_physical_charge() {
    let settings = settings(2, 100, 10);
    let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
    let mut outbox = outbox(
        &settings,
        owner.clone(),
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
    );
    enqueue(&mut outbox, "event", 0).unwrap();
    let (frame, progress) = outbox.begin_send(0).unwrap().unwrap();
    outbox.cancel();
    let mut retired = progress.retire(Duration::from_nanos(10));
    retired.accept(frame.len(), 5).unwrap();
    assert_eq!(retired.next_deadline().unwrap(), None);
    retired.on_timer(1_000).unwrap();
    assert_eq!(owner.used(), frame.len());
    drop(frame);
    assert_eq!(owner.used(), 0);
}

#[test]
fn zero_retired_progress_does_not_reset_original_send_deadline() {
    let settings = settings(2, 100, 10);
    let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
    let mut outbox = outbox(
        &settings,
        owner,
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
    );
    enqueue(&mut outbox, "event", 0).unwrap();
    let (_frame, progress) = outbox.begin_send(0).unwrap().unwrap();
    outbox.cancel();
    let mut retired = progress.retire(Duration::from_nanos(10));
    retired.accept(0, 5).unwrap();
    assert_eq!(retired.next_deadline().unwrap(), Some(10));
    assert_eq!(
        retired.on_timer(10),
        Err(SubscriptionOutboxSignal::SendStalled)
    );
}

#[test]
fn retired_progress_rejects_reversed_time_and_over_acceptance() {
    let settings = settings(2, 100, 10);
    let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
    let mut outbox = outbox(
        &settings,
        owner,
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
    );
    enqueue(&mut outbox, "event", 0).unwrap();
    let (frame, progress) = outbox.begin_send(0).unwrap().unwrap();
    outbox.cancel();
    let mut retired = progress.retire(Duration::from_nanos(10));
    retired.on_timer(5).unwrap();
    assert_eq!(
        retired.accept(1, 4),
        Err(SubscriptionOutboxSignal::NonMonotonicTime)
    );
    assert_eq!(
        retired.accept(frame.len() + 1, 6),
        Err(SubscriptionOutboxSignal::Accounting)
    );
}
