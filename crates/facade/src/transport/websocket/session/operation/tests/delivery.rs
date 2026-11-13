use std::sync::Arc;
use std::time::Duration;

use chigiri_http::SubscriptionTransport;
use chigiri_runtime::{GraphQlResponse, SubscriptionOverflow, Value};

use super::super::{WebSocketOperationSharedFailure, failure::is_shared_delivery};
use super::fixture::{Fixture, error};
use crate::subscription::outbox::{SubscriptionOutbox, SubscriptionOutboxSignal};
use crate::subscription::settings::{
    SubscriptionLimits, SubscriptionProtocolSettings, ValidatedSubscriptionSettings,
};
use crate::subscription::worker::websocket::WebSocketTerminalMessage;
use crate::transport::websocket::session::events::WebSocketEventOutbox;

#[test]
fn prepared_pressure_deadline_is_finite_and_expires_without_source_demand() {
    let settings = ValidatedSubscriptionSettings::new(
        SubscriptionLimits {
            max_queued_bytes: 32,
            max_encoded_event_bytes: 32,
            max_endpoint_encoded_bytes: 32,
            max_queue_age: Duration::from_nanos(7),
            ..SubscriptionLimits::default()
        },
        SubscriptionProtocolSettings::default(),
    );
    let bytes = crate::subscription::delivery::EndpointByteOwner::new(32);
    let _blocker = bytes.try_reserve(1).unwrap();
    let mut outbox = SubscriptionOutbox::new(
        &settings,
        bytes,
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::WebSocket,
        Some(Arc::from("pressure")),
    )
    .unwrap();

    assert_eq!(
        outbox.reserve_before_poll(5).err(),
        Some(SubscriptionOutboxSignal::Wait)
    );
    assert_eq!(outbox.next_deadline().unwrap(), Some(12));
    assert_eq!(
        outbox.on_timer(12),
        Err(SubscriptionOutboxSignal::SharedBudgetPressureExpired)
    );
}

#[test]
fn partial_physical_send_stall_is_shared_and_keeps_the_frame_owner() {
    let fixture = Fixture::new();
    let id: Arc<str> = Arc::from("partial");
    let active = fixture.active(Arc::clone(&id));
    let mut outbox = SubscriptionOutbox::new(
        &fixture.settings,
        fixture.bytes.clone(),
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::WebSocket,
        Some(Arc::clone(&id)),
    )
    .unwrap();
    let reservation = outbox.reserve_before_poll(0).unwrap();
    outbox
        .encode_and_enqueue(
            reservation,
            &GraphQlResponse::executed(Value::String("event".into())),
            0,
        )
        .unwrap();
    let mut events = WebSocketEventOutbox::new(
        outbox,
        active.websocket_physical_lease().unwrap(),
        active.emission_lease(),
        fixture.settings.limits().send_no_progress_timeout,
    )
    .unwrap();
    let carrier =
        crate::subscription::worker::websocket::WebSocketTerminalCarrier::joined_for_test(
            active,
            WebSocketTerminalMessage::Error(error()),
            &NeverDeadlines,
        )
        .unwrap();
    events.retire(&carrier).unwrap();
    let mut slot = fixture.slot_from_carrier(id, carrier);
    slot.events = Some(events);

    let frame = slot.begin_event(0).unwrap().expect("one queued event");
    slot.record_event_progress(1, false, 1).unwrap();
    let deadline = u64::try_from(
        fixture
            .settings
            .limits()
            .send_no_progress_timeout
            .as_nanos(),
    )
    .unwrap()
        + 1;
    assert_eq!(slot.next_deadline().unwrap(), Some(deadline));
    assert!(matches!(
        slot.begin_event(deadline),
        Err(WebSocketOperationSharedFailure::Delivery(
            SubscriptionOutboxSignal::SendStalled
        ))
    ));
    assert!(is_shared_delivery(SubscriptionOutboxSignal::SendStalled));
    assert!(!frame.as_bytes().is_empty());
}

struct NeverDeadlines;

impl chigiri_http::DeadlineFactory for NeverDeadlines {
    fn sleep(&self, _: Duration) -> chigiri_http::BoxFuture<'_, ()> {
        Box::pin(std::future::pending())
    }
}
