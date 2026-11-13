//! Direct handoff-state faults are not normal body cancellation.

use super::*;
use crate::subscription::bridge_resources::{
    ActiveBridgeOperation, BridgeOperationKind, BridgeResourceOwner,
};
use crate::subscription::control::HttpHeartbeatStaging;
use crate::subscription::settings::{
    SubscriptionLimits, SubscriptionProtocolSettings, ValidatedSubscriptionSettings,
};

mod waker_lifetime;

fn fixture() -> (
    BridgeResourceOwner,
    ActiveBridgeOperation,
    HttpSubscriptionPhysicalLease,
    Arc<BodyPort>,
) {
    let settings = ValidatedSubscriptionSettings::new(
        SubscriptionLimits::default(),
        SubscriptionProtocolSettings::default(),
    );
    let owner = BridgeResourceOwner::new(&settings).unwrap();
    let active = owner
        .begin_http_subscription()
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    let physical = active.http_physical_lease().unwrap();
    let port = BodyPort::new(Arc::new(StreamClock::new()));
    (owner, active, physical, port)
}

fn frame(owner: &BridgeResourceOwner) -> WireBytes {
    let mut heartbeat = HttpHeartbeatStaging::new(
        owner.delivery().byte_owner(),
        chigiri_http::SubscriptionTransport::ServerSentEvents,
    )
    .unwrap();
    heartbeat.mark_due().unwrap();
    WireBytes::Control(heartbeat.try_begin(false, false).unwrap().unwrap())
}

#[test]
fn double_offer_is_an_invariant_and_keeps_the_first_physical_owner() {
    let (owner, active, physical, port) = fixture();
    let bytes = owner.delivery().byte_owner();
    port.offer(frame(&owner), physical.clone()).unwrap();
    let first_charge = bytes.used();
    assert!(first_charge > 0);
    assert_eq!(
        port.offer(frame(&owner), physical.clone()),
        Err(HttpStreamingError::Invariant)
    );
    assert_eq!(bytes.used(), first_charge);
    assert!(port.state.lock().unwrap().offered.is_some());
    port.finish(None);
    assert_eq!(bytes.used(), 0);
    drop((physical, active));
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 0);
}

#[test]
fn offer_after_eof_is_an_invariant_not_cancellation() {
    let (owner, _active, physical, port) = fixture();
    port.finish(None);
    assert_eq!(
        port.offer(frame(&owner), physical),
        Err(HttpStreamingError::Invariant)
    );
    assert_eq!(owner.delivery().byte_owner().used(), 0);
    assert!(port.state.lock().unwrap().offered.is_none());
}

#[test]
fn offer_with_a_prior_error_is_an_invariant_even_without_eof() {
    let (owner, _active, physical, port) = fixture();
    // Isolate the error branch instead of letting EOF mask its condition.
    port.state.lock().unwrap().error = Some(HttpStreamingError::Worker);
    assert_eq!(
        port.offer(frame(&owner), physical),
        Err(HttpStreamingError::Invariant)
    );
    assert_eq!(owner.delivery().byte_owner().used(), 0);
    let state = port.state.lock().unwrap();
    assert_eq!(state.error, Some(HttpStreamingError::Worker));
    assert!(state.offered.is_none());
}

#[test]
fn actual_body_close_wins_as_clean_cancellation_and_drops_the_rejected_frame() {
    let (owner, _active, physical, port) = fixture();
    port.close_body();
    assert_eq!(
        port.offer(frame(&owner), physical),
        Err(HttpStreamingError::Closed)
    );
    assert_eq!(owner.delivery().byte_owner().used(), 0);
    assert!(port.state.lock().unwrap().offered.is_none());
}
