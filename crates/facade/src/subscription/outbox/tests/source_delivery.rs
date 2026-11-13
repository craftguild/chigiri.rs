//! Source and encoded evictions compose once at finalization and terminal freeze.

use chigiri_http::SubscriptionTransport;
use chigiri_runtime::__private::HostSourceDelivery;
use chigiri_runtime::SubscriptionOverflow;

use super::fixture::{EVENT_LIMIT, counter, enqueue, outbox, settings};
use crate::subscription::delivery::EndpointByteOwner;
const MAX_EXACT_JSON_INTEGER: u64 = (1_u64 << 53) - 1;
use crate::subscription::outbox::SubscriptionOutboxSignal;

#[test]
fn two_stage_source_delivery_is_exact_and_an_in_flight_frame_is_immutable() {
    for transport in [
        SubscriptionTransport::WebSocket,
        SubscriptionTransport::ServerSentEvents,
        SubscriptionTransport::Multipart,
    ] {
        let mut outbox = outbox(
            &settings(1, 100, 100),
            EndpointByteOwner::new(16 * EVENT_LIMIT),
            SubscriptionOverflow::DropOldest,
            transport,
        );
        let source = HostSourceDelivery::new();
        outbox.attach_source_delivery(Some(source.clone()));
        enqueue(&mut outbox, "encoded-drop", 0).unwrap();
        enqueue(&mut outbox, "survivor", 1).unwrap();
        assert_eq!(source.record_drop(), Some(1));
        assert_eq!(source.record_drop(), Some(2));
        let (frame, mut progress) = outbox.begin_send(2).unwrap().unwrap();
        assert_eq!(counter(frame.as_bytes()), Some(3));
        assert_eq!(source.record_drop(), Some(3));
        assert_eq!(counter(frame.as_bytes()), Some(3));
        assert!(progress.accept(frame.len(), 3).unwrap());
        drop(frame);
        outbox.finish_send(progress, 3).unwrap();
        enqueue(&mut outbox, "next", 4).unwrap();
        let (frame, _) = outbox.begin_send(5).unwrap().unwrap();
        assert_eq!(counter(frame.as_bytes()), Some(4));
        assert_eq!(
            outbox.take_drop_observations().collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );
        assert!(outbox.take_drop_observations().next().is_none());
        assert_eq!(outbox.freeze_terminal_counter(), 4);
        assert_eq!(outbox.freeze_terminal_counter(), 4);
        assert_eq!(source.record_drop(), None);
        assert_eq!(source.snapshot(), Some(3));
        assert_eq!(counter(frame.as_bytes()), Some(4));
    }
}

#[test]
fn source_delivery_counter_exhaustion_terminates_before_combined_precision_loss() {
    let mut outbox = outbox(
        &settings(1, 100, 100),
        EndpointByteOwner::new(16 * EVENT_LIMIT),
        SubscriptionOverflow::DropOldest,
        SubscriptionTransport::ServerSentEvents,
    );
    outbox
        .controller
        .as_mut()
        .unwrap()
        .set_dropped_for_test(MAX_EXACT_JSON_INTEGER - 1);
    let source = HostSourceDelivery::new();
    source.record_drop().unwrap();
    source.record_drop().unwrap();
    outbox.attach_source_delivery(Some(source.clone()));
    assert_eq!(
        outbox.on_timer(0),
        Err(SubscriptionOutboxSignal::CounterExhausted)
    );
    assert_eq!(outbox.delivery_counter(), Some(MAX_EXACT_JSON_INTEGER - 1));
    assert!(source.is_stopped());
}

#[test]
fn large_source_delivery_jumps_keep_observation_work_bounded() {
    let mut outbox = outbox(
        &settings(1, 100, 100),
        EndpointByteOwner::new(16 * EVENT_LIMIT),
        SubscriptionOverflow::DropOldest,
        SubscriptionTransport::ServerSentEvents,
    );
    let source = HostSourceDelivery::new();
    let total = crate::SubscriptionLimits::MAX_QUEUED_EVENTS as u64 + 2;
    for _ in 0..total {
        source.record_drop().unwrap();
    }
    outbox.attach_source_delivery(Some(source));
    outbox.on_timer(0).unwrap();
    assert_eq!(outbox.take_drop_observations().collect::<Vec<_>>(), [total]);
    enqueue(&mut outbox, "value", 1).unwrap();
    let (frame, _) = outbox.begin_send(2).unwrap().unwrap();
    assert_eq!(counter(frame.as_bytes()), Some(total));
}

#[test]
fn non_dropping_source_delivery_preserves_null_and_rejects_host_policy_mismatch() {
    let mut outbox = outbox(
        &settings(1, 100, 100),
        EndpointByteOwner::new(16 * EVENT_LIMIT),
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
    );
    let source = HostSourceDelivery::new();
    outbox.attach_source_delivery(Some(source.clone()));
    enqueue(&mut outbox, "value", 0).unwrap();
    let (frame, _) = outbox.begin_send(1).unwrap().unwrap();
    assert_eq!(counter(frame.as_bytes()), None);
    source.record_drop().unwrap();
    assert_eq!(outbox.on_timer(2), Err(SubscriptionOutboxSignal::Invariant));
    assert!(source.is_stopped());
}
