use std::sync::Arc;
use std::task::Poll;

use chigiri_http::SubscriptionTransport;
use chigiri_runtime::{GraphQlResponse, SubscriptionOverflow, Value};

use super::super::{WebSocketOperationTransition, failure::WebSocketOperationSharedFailure};
use super::fixture::{Fixture, context, error};
use crate::subscription::outbox::SubscriptionOutbox;
use crate::subscription::worker::websocket::WebSocketTerminalMessage;
use crate::transport::websocket::session::events::WebSocketEventOutbox;

#[test]
fn terminal_releases_id_then_transfers_control_before_slot_reap() {
    let mut fixture = Fixture::new();
    let id: Arc<str> = Arc::from("same");
    let active = fixture.active(Arc::clone(&id));
    let old_generation = active.generation();
    let mut slot = fixture.slot(
        Arc::clone(&id),
        active,
        WebSocketTerminalMessage::Error(error()),
    );

    let replacement = fixture.connection.begin(Arc::clone(&id)).unwrap();
    assert_ne!(replacement.generation(), old_generation);
    assert!(matches!(
        poll(&mut slot, &mut fixture, 0),
        Poll::Ready(Ok(WebSocketOperationTransition::Released { id: released, generation }))
            if released == id && generation == old_generation
    ));
    assert!(matches!(
        poll(&mut slot, &mut fixture, 0),
        Poll::Ready(Ok(WebSocketOperationTransition::Finished))
    ));
    assert!(slot.is_finished());
    assert_eq!(slot.wire_id(), None);
    assert_eq!(fixture.controls.queued_messages(), 1);

    fixture.controls.discard_operation(&id).unwrap();
    assert_eq!(fixture.controls.queued_messages(), 0);
    drop(replacement);
}

#[test]
fn client_cancel_suppresses_unsent_terminal_and_does_not_release_twice() {
    let mut fixture = Fixture::new();
    let id: Arc<str> = Arc::from("cancel");
    let active = fixture.active(Arc::clone(&id));
    let mut slot = fixture.slot(
        Arc::clone(&id),
        active,
        WebSocketTerminalMessage::Error(error()),
    );

    assert!(matches!(
        poll(&mut slot, &mut fixture, 0),
        Poll::Ready(Ok(WebSocketOperationTransition::Released { .. }))
    ));
    assert!(matches!(
        slot.cancel(&mut fixture.controls),
        Ok(WebSocketOperationTransition::Progress)
    ));
    assert_eq!(slot.wire_id(), None);
    assert!(matches!(
        poll(&mut slot, &mut fixture, 0),
        Poll::Ready(Ok(WebSocketOperationTransition::Finished))
    ));
    assert_eq!(fixture.controls.queued_messages(), 0);
    assert!(fixture.connection.begin(id).is_ok());
}

#[test]
fn disconnect_drops_slot_progress_but_not_the_handed_off_frame_owner() {
    let mut fixture = Fixture::new();
    let id: Arc<str> = Arc::from("disconnect");
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
    let physical = active.websocket_physical_lease().unwrap();
    let emission = active.emission_lease();
    let mut events = WebSocketEventOutbox::new(
        outbox,
        physical,
        emission,
        fixture.settings.limits().send_no_progress_timeout,
    )
    .unwrap();
    let carrier =
        crate::subscription::worker::websocket::WebSocketTerminalCarrier::joined_for_test(
            active,
            WebSocketTerminalMessage::Complete,
            &NeverDeadlines,
        )
        .unwrap();
    events.retire(&carrier).unwrap();
    let mut slot = fixture.slot_from_carrier(id, carrier);
    slot.events = Some(events);

    let frame = slot.begin_event(0).unwrap().expect("one handed-off frame");
    assert!(!frame.as_bytes().is_empty());
    assert!(matches!(
        slot.disconnect(&mut fixture.controls),
        Ok(WebSocketOperationTransition::Released { .. })
    ));
    assert!(!slot.has_event());
    assert!(!frame.as_bytes().is_empty());
    assert!(matches!(
        poll(&mut slot, &mut fixture, 0),
        Poll::Ready(Ok(WebSocketOperationTransition::Finished))
    ));
    assert_eq!(fixture.controls.queued_messages(), 0);
    drop(frame);
}

fn poll(
    slot: &mut super::super::WebSocketOperationSlot<()>,
    fixture: &mut Fixture,
    now: u64,
) -> Poll<Result<WebSocketOperationTransition, WebSocketOperationSharedFailure>> {
    slot.poll(&mut context(), now, &mut fixture.controls)
}

struct NeverDeadlines;

impl chigiri_http::DeadlineFactory for NeverDeadlines {
    fn sleep(&self, _: std::time::Duration) -> chigiri_http::BoxFuture<'_, ()> {
        Box::pin(std::future::pending())
    }
}
