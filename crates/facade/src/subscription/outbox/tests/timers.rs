//! Timer, progress, and endpoint-pressure behavior.

use chigiri_http::SubscriptionTransport;
use chigiri_runtime::SubscriptionOverflow;

use super::super::SubscriptionOutboxSignal;
use super::fixture::{EVENT_LIMIT, counter, enqueue, outbox, settings};
use crate::subscription::delivery::EndpointByteOwner;

#[test]
fn zero_progress_never_resets_the_send_deadline() {
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
    assert_eq!(outbox.next_deadline().unwrap(), Some(10));
    assert!(!progress.accept(0, 5).unwrap());
    outbox.on_progress(5).unwrap();
    assert_eq!(outbox.next_deadline().unwrap(), Some(10));
    assert_eq!(
        outbox.on_timer(10).unwrap_err(),
        SubscriptionOutboxSignal::SendStalled
    );
    assert!(owner.used() > 0);
    assert_eq!(
        progress.accepted_bytes().unwrap_err(),
        SubscriptionOutboxSignal::SendStalled
    );
    drop(frame);
    assert_eq!(owner.used(), 0);
}

#[test]
fn positive_progress_resets_the_deadline_and_completed_bytes_disable_it() {
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
    outbox.on_progress(5).unwrap();
    assert_eq!(outbox.next_deadline().unwrap(), Some(15));
    assert!(progress.accept(frame.len() - 1, 6).unwrap());
    outbox.on_progress(6).unwrap();
    assert_eq!(outbox.next_deadline().unwrap(), None);
    assert!(owner.used() > 0);
    drop(frame);
    outbox.finish_send(progress, 6).unwrap();
    assert_eq!(owner.used(), 0);
}

#[test]
fn queue_age_acts_without_new_input_for_both_policies() {
    let settings = settings(3, 10, 100);
    let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
    let mut backpressure = outbox(
        &settings,
        owner.clone(),
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
    );
    enqueue(&mut backpressure, "expired", 0).unwrap();
    assert_eq!(backpressure.next_deadline().unwrap(), Some(10));
    assert_eq!(
        backpressure.on_timer(10).unwrap_err(),
        SubscriptionOutboxSignal::QueueAgeExpired
    );

    let mut dropping = outbox(
        &settings,
        owner,
        SubscriptionOverflow::DropOldest,
        SubscriptionTransport::ServerSentEvents,
    );
    enqueue(&mut dropping, "expired", 0).unwrap();
    enqueue(&mut dropping, "survivor", 5).unwrap();
    dropping.on_timer(10).unwrap();
    let (frame, _) = dropping.begin_send(10).unwrap().unwrap();
    assert!(
        std::str::from_utf8(frame.as_bytes())
            .unwrap()
            .contains("survivor")
    );
    assert_eq!(counter(frame.as_bytes()), Some(1));
}

#[test]
fn shared_pressure_deadline_acts_without_source_input() {
    let settings = settings(3, 10, 100);
    let owner = EndpointByteOwner::new(EVENT_LIMIT);
    let mut holder = outbox(
        &settings,
        owner.clone(),
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
    );
    let mut waiting = outbox(
        &settings,
        owner,
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
    );
    let _held = holder.reserve_before_poll(0).unwrap();
    assert_eq!(
        waiting.reserve_before_poll(1).err().unwrap(),
        SubscriptionOutboxSignal::Wait
    );
    assert_eq!(waiting.next_deadline().unwrap(), Some(11));
    assert_eq!(
        waiting.on_timer(11).unwrap_err(),
        SubscriptionOutboxSignal::SharedBudgetPressureExpired
    );
}
