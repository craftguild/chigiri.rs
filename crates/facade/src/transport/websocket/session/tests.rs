//! Actual original frame ownership; no WebSocket-library or network simulation.

mod writer;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};

use chigiri_http::SubscriptionTransport;
use chigiri_runtime::{GraphQlResponse, SubscriptionOverflow, Value};

use super::WebSocketWireFrame;
use crate::subscription::bridge_resources::{BridgeOperationKind, BridgeResourceOwner};
use crate::subscription::outbox::{SubscriptionOutbox, SubscriptionSendProgress};
use crate::subscription::settings::{
    SubscriptionLimits, SubscriptionProtocolSettings, ValidatedSubscriptionSettings,
};

fn event() -> (
    Arc<BridgeResourceOwner>,
    SubscriptionOutbox,
    SubscriptionSendProgress,
    WebSocketWireFrame,
) {
    let settings = ValidatedSubscriptionSettings::new(
        SubscriptionLimits::default(),
        SubscriptionProtocolSettings::default(),
    );
    let owner = Arc::new(BridgeResourceOwner::new(&settings).unwrap());
    let connection = owner.open_connection().unwrap();
    let active = connection
        .begin(Arc::from("event-id"))
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    let physical = active.websocket_physical_lease().unwrap();
    let mut outbox = SubscriptionOutbox::new(
        &settings,
        owner.delivery().byte_owner(),
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::WebSocket,
        Some(Arc::from("event-id")),
    )
    .unwrap();
    let reservation = outbox.reserve_before_poll(0).unwrap();
    let response = GraphQlResponse::executed(Value::String("private-event-value".into()));
    outbox
        .encode_and_enqueue(reservation, &response, 0)
        .unwrap();
    let (frame, progress) = outbox.begin_send(0).unwrap().unwrap();
    let wire = WebSocketWireFrame::event(frame, physical);
    // No worker was started. These owners finish synchronously, leaving only
    // the immutable wire frame's original operation/connection and byte charge.
    drop(active.revoke().unwrap());
    drop(connection);
    (owner, outbox, progress, wire)
}

#[test]
fn event_clones_retain_original_charges_until_last_drop_then_allow_finish() {
    let (owner, mut outbox, mut progress, wire) = event();
    let release = wire.release();
    let clone = wire.clone();
    let length = wire.as_bytes().len();
    assert_eq!(wire.as_bytes().as_ptr(), clone.as_bytes().as_ptr());
    assert!(
        wire.as_bytes()
            .starts_with(br#"{"id":"event-id","type":"next""#)
    );
    assert!(!format!("{wire:?}").contains("private-event-value"));
    assert_eq!(owner.delivery().byte_owner().used(), length);
    assert_eq!(owner.delivery().snapshot().endpoint_connections, 1);
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 1);
    assert!(progress.accept(length, 1).unwrap());
    let mut context = Context::from_waker(Waker::noop());
    assert!(release.poll(&mut context).is_pending());
    drop(wire);
    assert!(release.poll(&mut context).is_pending());
    assert_eq!(owner.delivery().byte_owner().used(), length);
    drop(clone);
    assert_eq!(release.poll(&mut context), Poll::Ready(Ok(())));
    // The controller still owns its matching in-flight entry until finish_send.
    // The release notification owns no bytes and is not that final transition.
    assert_eq!(owner.delivery().byte_owner().used(), length);
    assert_eq!(owner.delivery().snapshot().endpoint_connections, 0);
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 0);
    outbox.finish_send(progress, 2).unwrap();
    assert_eq!(owner.delivery().byte_owner().used(), 0);
    // Keeping the release waiter cannot retain any of the released capacity.
    drop(release);
    assert_eq!(owner.delivery().byte_owner().used(), 0);
}

struct ReleasedProbe {
    owner: Arc<BridgeResourceOwner>,
    wakes: AtomicUsize,
}

impl Wake for ReleasedProbe {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        // A wake may synchronously reenter admission. All original charges
        // must already be released, not merely scheduled for destruction.
        assert_eq!(self.owner.delivery().byte_owner().used(), 0);
        let snapshot = self.owner.delivery().snapshot();
        assert_eq!(snapshot.endpoint_active_subscriptions, 0);
        assert_eq!(snapshot.endpoint_connections, 0);
        self.wakes.fetch_add(1, Ordering::SeqCst);
        drop(self.owner.open_connection().unwrap());
    }
}

#[test]
fn last_reference_wakes_after_payload_charge_and_connection_are_destroyed() {
    let (owner, outbox, _progress, wire) = event();
    // Cancellation has dropped the independent controller owner. The wire is
    // now the only remaining owner of the immutable payload and its byte charge.
    drop(outbox);
    let release = wire.release();
    let probe = Arc::new(ReleasedProbe {
        owner,
        wakes: AtomicUsize::new(0),
    });
    let waker = Waker::from(Arc::clone(&probe));
    let mut context = Context::from_waker(&waker);
    assert!(release.poll(&mut context).is_pending());
    let first = wire.clone();
    let last = wire.clone();
    drop(wire);
    drop(first);
    assert_eq!(probe.wakes.load(Ordering::SeqCst), 0);
    drop(last);
    assert_eq!(probe.wakes.load(Ordering::SeqCst), 1);
    assert_eq!(release.poll(&mut context), Poll::Ready(Ok(())));
}

#[test]
fn dropping_before_a_waiter_is_polled_does_not_lose_release() {
    let (owner, outbox, _progress, wire) = event();
    drop(outbox);
    let release = wire.release();
    drop(wire);
    assert_eq!(
        release.poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Ok(()))
    );
    assert_eq!(owner.delivery().byte_owner().used(), 0);
}
