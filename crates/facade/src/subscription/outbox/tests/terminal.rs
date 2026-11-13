//! HTTP source retirement, protocol projection, and physical control ownership.

use std::sync::Arc;

use chigiri_http::SubscriptionTransport;
use chigiri_runtime::{GraphQlError, GraphQlResponse, Location, PathSegment, SubscriptionOverflow};

use super::super::{SubscriptionOutbox, SubscriptionOutboxSignal};
use super::fixture::{EVENT_LIMIT, counter, enqueue, outbox, settings};
use crate::subscription::control::{HttpTerminalStaging, SubscriptionControlSignal};
use crate::subscription::delivery::EndpointByteOwner;
use crate::subscription::settings::ValidatedSubscriptionSettings;

fn profiles() -> [SubscriptionTransport; 2] {
    [
        SubscriptionTransport::ServerSentEvents,
        SubscriptionTransport::Multipart,
    ]
}

fn complete_bytes(transport: SubscriptionTransport) -> &'static [u8] {
    match transport {
        SubscriptionTransport::ServerSentEvents => b"event: complete\ndata: \n\n",
        SubscriptionTransport::Multipart => b"--graphql--",
        _ => unreachable!(),
    }
}

fn error_response() -> GraphQlResponse {
    let mut error = GraphQlError::new("safe source failure").code("SOURCE_FAILED");
    error.locations.push(Location { line: 3, column: 8 });
    error.path.push(PathSegment::Field("chosen".to_owned()));
    GraphQlResponse::request_error(error)
}

fn error_terminal(outbox: &mut SubscriptionOutbox) -> HttpTerminalStaging {
    let reservation = outbox.reserve_before_poll(10).unwrap();
    outbox
        .finish_http_source(reservation)
        .unwrap()
        .error(&error_response())
        .unwrap()
}

#[test]
fn natural_completion_reuses_source_charge_and_waits_for_last_physical_reference() {
    for transport in profiles() {
        let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
        let mut outbox = outbox(
            &settings(2, 100, 100),
            owner.clone(),
            SubscriptionOverflow::Backpressure,
            transport,
        );
        let reservation = outbox.reserve_before_poll(0).unwrap();
        assert_eq!(owner.used(), EVENT_LIMIT);
        let mut terminal = outbox
            .finish_http_source(reservation)
            .unwrap()
            .complete()
            .unwrap();
        assert_eq!(owner.used(), complete_bytes(transport).len());
        assert_eq!(
            outbox.reserve_before_poll(1).err().unwrap(),
            SubscriptionOutboxSignal::Cancelled
        );
        let frame = Arc::new(terminal.begin_send().unwrap().unwrap());
        assert_eq!(frame.as_bytes(), complete_bytes(transport));
        let last = Arc::clone(&frame);
        drop(frame);
        assert!(!terminal.is_finished());
        assert!(terminal.begin_send().unwrap().is_none());
        assert_eq!(owner.used(), last.len());
        drop(last);
        assert!(terminal.is_finished());
        assert_eq!(owner.used(), 0);
    }
}

#[test]
fn source_error_has_current_counter_and_projection_then_a_separate_completion() {
    for transport in profiles() {
        for overflow in [
            SubscriptionOverflow::Backpressure,
            SubscriptionOverflow::DropOldest,
        ] {
            let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
            let mut outbox = outbox(&settings(2, 100, 100), owner.clone(), overflow, transport);
            let mut terminal = error_terminal(&mut outbox);
            let frame = Arc::new(terminal.begin_send().unwrap().unwrap());
            let text = std::str::from_utf8(frame.as_ref().as_bytes()).unwrap();
            assert!(text.contains("safe source failure"));
            assert!(text.contains("SOURCE_FAILED"));
            if transport == SubscriptionTransport::ServerSentEvents {
                assert!(text.starts_with("event: next\ndata: "));
                assert!(text.contains("\"locations\""));
                assert!(text.contains("\"path\""));
                assert_eq!(
                    counter(frame.as_bytes()),
                    (overflow == SubscriptionOverflow::DropOldest).then_some(0)
                );
            } else {
                assert!(text.contains("\"payload\":null,\"errors\":"));
                assert!(!text.contains("\"locations\""));
                assert!(!text.contains("\"path\""));
                assert!(!text.contains("droppedTotal"));
            }
            assert!(!text.contains(std::str::from_utf8(complete_bytes(transport)).unwrap()));
            let last = Arc::clone(&frame);
            drop(frame);
            assert!(terminal.begin_send().unwrap().is_none());
            drop(last);
            let complete = terminal.begin_send().unwrap().unwrap();
            assert_eq!(complete.as_bytes(), complete_bytes(transport));
            assert_eq!(owner.used(), complete.len());
            drop(complete);
            assert!(terminal.is_finished());
            assert_eq!(owner.used(), 0);
        }
    }
}

#[test]
fn terminal_counter_includes_real_prior_evictions_without_evicting_terminal_controls() {
    let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
    let mut outbox = outbox(
        &settings(2, 100, 100),
        owner.clone(),
        SubscriptionOverflow::DropOldest,
        SubscriptionTransport::ServerSentEvents,
    );
    for (time, value) in ["evicted", "second", "third"].into_iter().enumerate() {
        enqueue(&mut outbox, value, time as u64).unwrap();
    }
    for now in [3, 4] {
        let (frame, mut progress) = outbox.begin_send(now).unwrap().unwrap();
        assert!(progress.accept(frame.len(), now).unwrap());
        drop(frame);
        outbox.finish_send(progress, now).unwrap();
    }
    let mut terminal = error_terminal(&mut outbox);
    let frame = terminal.begin_send().unwrap().unwrap();
    assert_eq!(counter(frame.as_bytes()), Some(1));
    assert_eq!(
        outbox.reserve_before_poll(11).err().unwrap(),
        SubscriptionOutboxSignal::Cancelled
    );
    terminal.cancel();
    assert_eq!(owner.used(), frame.len());
    drop(frame);
    assert_eq!(owner.used(), 0);
}

#[test]
fn foreign_source_reservation_and_websocket_profile_cannot_compose_http_controls() {
    let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
    let settings = settings(2, 100, 100);
    let mut first = outbox(
        &settings,
        owner.clone(),
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
    );
    let mut second = outbox(
        &settings,
        owner.clone(),
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::ServerSentEvents,
    );
    let foreign = first.reserve_before_poll(0).unwrap();
    assert_eq!(
        second.finish_http_source(foreign).err().unwrap(),
        SubscriptionOutboxSignal::Invariant
    );
    assert!(first.reserve_before_poll(1).is_ok());
    assert_eq!(owner.used(), 0);
    let mut websocket = outbox(
        &settings,
        owner.clone(),
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::WebSocket,
    );
    let reservation = websocket.reserve_before_poll(0).unwrap();
    assert_eq!(
        websocket.finish_http_source(reservation).err().unwrap(),
        SubscriptionOutboxSignal::InvalidTransportArguments
    );
    assert_eq!(owner.used(), 0);
}

#[test]
fn earlier_queued_or_physical_event_cannot_be_silently_replaced_by_completion() {
    for physical in [false, true] {
        let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
        let mut outbox = outbox(
            &settings(2, 100, 100),
            owner.clone(),
            SubscriptionOverflow::DropOldest,
            SubscriptionTransport::Multipart,
        );
        enqueue(&mut outbox, "earlier", 0).unwrap();
        let sent = physical.then(|| outbox.begin_send(1).unwrap().unwrap());
        let reservation = outbox.reserve_before_poll(2).unwrap();
        assert_eq!(
            outbox.finish_http_source(reservation).err().unwrap(),
            SubscriptionOutboxSignal::Invariant
        );
        if let Some((frame, _)) = &sent {
            assert_eq!(owner.used(), frame.len());
        } else {
            assert_eq!(owner.used(), 0);
        }
        drop(sent);
        assert_eq!(owner.used(), 0);
    }
}

#[test]
fn failed_encoding_and_cancelled_staging_never_publish_uncharged_or_partial_controls() {
    for transport in profiles() {
        let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
        let mut outbox = outbox(
            &settings(2, 100, 100),
            owner.clone(),
            SubscriptionOverflow::Backpressure,
            transport,
        );
        let reservation = outbox.reserve_before_poll(0).unwrap();
        let terminal = outbox.finish_http_source(reservation).unwrap();
        let oversized =
            GraphQlResponse::request_error(GraphQlError::new("x".repeat(2 * EVENT_LIMIT)));
        assert!(terminal.error(&oversized).is_err());
        assert_eq!(owner.used(), 0);
        let mut fresh = super::fixture::outbox(
            &settings(2, 100, 100),
            owner.clone(),
            SubscriptionOverflow::Backpressure,
            transport,
        );
        let mut terminal = error_terminal(&mut fresh);
        assert!(owner.used() > 0);
        terminal.cancel();
        assert_eq!(
            terminal.begin_send().err().unwrap(),
            SubscriptionControlSignal::Cancelled
        );
        assert!(terminal.is_finished());
        assert_eq!(owner.used(), 0);
    }
}

#[test]
fn deferred_completion_reserves_before_encoding_and_capacity_failure_is_sticky() {
    for transport in profiles() {
        let owner = EndpointByteOwner::new(16 * EVENT_LIMIT);
        let mut outbox = outbox(
            &settings(2, 100, 100),
            owner.clone(),
            SubscriptionOverflow::Backpressure,
            transport,
        );
        let mut terminal = error_terminal(&mut outbox);
        drop(terminal.begin_send().unwrap().unwrap());
        let held = owner.try_reserve(owner.limit()).unwrap();
        assert_eq!(
            terminal.begin_send().err().unwrap(),
            SubscriptionControlSignal::SharedByteCapacity
        );
        drop(held);
        assert_eq!(
            terminal.begin_send().err().unwrap(),
            SubscriptionControlSignal::Cancelled
        );
        terminal.cancel();
        assert!(terminal.is_finished());
        assert_eq!(owner.used(), 0);
    }
}

#[test]
fn natural_completion_can_replace_a_tiny_event_reservation_with_its_exact_static_size() {
    for transport in profiles() {
        let settings = settings(2, 100, 100);
        let mut limits = *settings.limits();
        limits.max_encoded_event_bytes = 1;
        let settings = ValidatedSubscriptionSettings::new(limits, *settings.protocol());
        let owner = EndpointByteOwner::new(64);
        let mut outbox = outbox(
            &settings,
            owner.clone(),
            SubscriptionOverflow::Backpressure,
            transport,
        );
        let reservation = outbox.reserve_before_poll(0).unwrap();
        assert_eq!(owner.used(), 1);
        let mut terminal = outbox
            .finish_http_source(reservation)
            .unwrap()
            .complete()
            .unwrap();
        let frame = terminal.begin_send().unwrap().unwrap();
        assert_eq!(frame.as_bytes(), complete_bytes(transport));
        assert_eq!(owner.used(), frame.len());
        drop(frame);
        assert!(terminal.is_finished());
        assert_eq!(owner.used(), 0);
    }
}
