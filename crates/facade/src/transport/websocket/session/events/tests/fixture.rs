use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use chigiri_http::{BoxFuture, DeadlineFactory, SubscriptionTransport};
use chigiri_runtime::{GraphQlResponse, SubscriptionOverflow, Value};

use super::super::WebSocketEventOutbox;
use crate::subscription::bridge_resources::{
    ActiveBridgeOperation, BridgeCleanupOutcome, BridgeConnection, BridgeOperationKind,
    BridgeResourceOwner,
};
use crate::subscription::outbox::SubscriptionOutbox;
use crate::subscription::settings::{
    SubscriptionLimits, SubscriptionProtocolSettings, ValidatedSubscriptionSettings,
};
use crate::subscription::worker::websocket::{WebSocketTerminalCarrier, WebSocketTerminalMessage};

pub(super) struct Fixture {
    pub(super) owner: BridgeResourceOwner,
    pub(super) connection: BridgeConnection,
    pub(super) active: Option<ActiveBridgeOperation>,
    pub(super) queue: WebSocketEventOutbox,
}

pub(super) fn fixture() -> Fixture {
    let limits = SubscriptionLimits {
        max_queued_events: 3,
        max_queued_bytes: 8 * 1024,
        max_encoded_event_bytes: 1024,
        max_endpoint_encoded_bytes: 16 * 1024,
        max_queue_age: Duration::from_nanos(100),
        send_no_progress_timeout: Duration::from_nanos(10),
        ..SubscriptionLimits::default()
    };
    let settings =
        ValidatedSubscriptionSettings::new(limits, SubscriptionProtocolSettings::default());
    let owner = BridgeResourceOwner::new(&settings).unwrap();
    let connection = owner.open_connection().unwrap();
    let active = connection
        .begin(Arc::from("same"))
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    let outbox = SubscriptionOutbox::new(
        &settings,
        owner.delivery().byte_owner(),
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::WebSocket,
        Some(Arc::from("same")),
    )
    .unwrap();
    let queue = WebSocketEventOutbox::new(
        outbox,
        active.websocket_physical_lease().unwrap(),
        active.emission_lease(),
        Duration::from_nanos(10),
    )
    .unwrap();
    Fixture {
        owner,
        connection,
        active: Some(active),
        queue,
    }
}

pub(super) fn response(value: &str) -> GraphQlResponse {
    GraphQlResponse::executed(Value::String(value.into()))
}

pub(super) fn enqueue(queue: &mut WebSocketEventOutbox, value: &str, now: u64) {
    let reservation = queue.reserve_before_poll(now).unwrap();
    queue
        .encode_and_enqueue(reservation, &response(value), now)
        .unwrap();
}

struct Deadlines;

impl DeadlineFactory for Deadlines {
    fn sleep(&self, _: Duration) -> BoxFuture<'_, ()> {
        Box::pin(std::future::pending())
    }
}

pub(super) fn terminal(
    active: ActiveBridgeOperation,
    message: WebSocketTerminalMessage,
) -> WebSocketTerminalCarrier {
    WebSocketTerminalCarrier::joined_for_test(active, message, &Deadlines).unwrap()
}

pub(super) fn finish_cleanup(terminal: &mut WebSocketTerminalCarrier) {
    assert_eq!(
        terminal.poll_cleanup(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(BridgeCleanupOutcome::Completed)
    );
}
