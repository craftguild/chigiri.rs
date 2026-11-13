use std::sync::Arc;

use chigiri_http::SubscriptionStaticControl;

use super::super::{SubscriptionControlSignal, WebSocketControlClass, WebSocketControlStaging};
use super::fixture::error_response;
use crate::subscription::bridge_resources::{
    BridgeOperationKind, BridgeResourceError, BridgeResourceOwner,
};
use crate::subscription::settings::{
    SubscriptionLimits, SubscriptionProtocolSettings, ValidatedSubscriptionSettings,
};

fn owner(active: usize) -> BridgeResourceOwner {
    BridgeResourceOwner::new(&ValidatedSubscriptionSettings::new(
        SubscriptionLimits::default(),
        SubscriptionProtocolSettings {
            max_active_operations_per_connection: active,
            ..SubscriptionProtocolSettings::default()
        },
    ))
    .unwrap()
}

#[test]
fn old_generation_pruning_preserves_replacement_and_unrelated_control_order() {
    let resources = owner(2);
    let bytes = resources.delivery().byte_owner();
    let mut queue = WebSocketControlStaging::new(bytes.clone());
    let connection = resources.open_connection().unwrap();
    let old = connection
        .begin(Arc::from("same"))
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    let old_generation = old.generation();
    queue
        .enqueue_active_complete(Arc::from("same"), old.websocket_physical_lease().unwrap())
        .unwrap();
    drop(old.revoke().unwrap());
    let replacement = connection
        .begin(Arc::from("same"))
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    let new_generation = replacement.generation();
    assert_ne!(old_generation, new_generation);
    queue
        .enqueue_static(SubscriptionStaticControl::Heartbeat)
        .unwrap();
    queue
        .enqueue_active_error(
            Arc::from("same"),
            replacement.websocket_physical_lease().unwrap(),
            &error_response("new"),
        )
        .unwrap();
    assert_eq!(
        resources
            .delivery()
            .snapshot()
            .endpoint_active_subscriptions,
        2
    );
    queue.discard_generation(old_generation).unwrap();
    assert_eq!(
        resources
            .delivery()
            .snapshot()
            .endpoint_active_subscriptions,
        1
    );
    assert_eq!(queue.queued_messages(), 2);
    assert_eq!(bytes.used(), queue.queued_bytes());
    let frame = queue
        .begin_send_for(WebSocketControlClass::Priority)
        .unwrap()
        .unwrap();
    assert_eq!(frame.websocket_generation(), Some(new_generation));
    assert_eq!(frame.websocket_operation_id(), Some("same"));
    assert!(
        std::str::from_utf8(frame.as_bytes())
            .unwrap()
            .contains("new")
    );
    // A late old cleanup cannot touch the new generation or its physical frame.
    queue.discard_generation(old_generation).unwrap();
    assert_eq!(queue.queued_messages(), 1);
    drop(replacement.revoke().unwrap());
    assert_eq!(
        resources
            .delivery()
            .snapshot()
            .endpoint_active_subscriptions,
        1
    );
    drop(frame);
    assert_eq!(
        resources
            .delivery()
            .snapshot()
            .endpoint_active_subscriptions,
        0
    );
    let ping = queue
        .begin_send_for(WebSocketControlClass::Ordinary)
        .unwrap()
        .unwrap();
    assert_eq!(ping.as_bytes(), br#"{"type":"ping"}"#);
    assert_eq!(ping.websocket_generation(), None);
    drop(ping);
    assert_eq!(bytes.used(), 0);
}

#[test]
fn queued_and_last_physical_terminal_clones_hold_the_original_charge_for_every_kind() {
    for kind in [
        BridgeOperationKind::Query,
        BridgeOperationKind::Mutation,
        BridgeOperationKind::Subscription,
    ] {
        let resources = owner(1);
        let bytes = resources.delivery().byte_owner();
        let mut queue = WebSocketControlStaging::new(bytes.clone());
        let connection = resources.open_connection().unwrap();
        let active = connection
            .begin(Arc::from("id"))
            .unwrap()
            .activate(kind)
            .unwrap();
        let generation = active.generation();
        queue
            .enqueue_active_complete(Arc::from("id"), active.websocket_physical_lease().unwrap())
            .unwrap();
        drop(active.revoke().unwrap());
        let try_replacement = || connection.begin(Arc::from("id")).unwrap().activate(kind);
        assert_eq!(try_replacement().err(), Some(BridgeResourceError::Capacity));
        let frame = Arc::new(
            queue
                .begin_send_for(WebSocketControlClass::Priority)
                .unwrap()
                .unwrap(),
        );
        assert_eq!(frame.websocket_generation(), Some(generation));
        let held = Arc::clone(&frame);
        // Client completion only cancels wholly unsent entries, not this frame.
        queue.discard_operation("id").unwrap();
        queue.cancel();
        assert_eq!(bytes.used(), frame.len());
        drop(frame);
        assert_eq!(try_replacement().err(), Some(BridgeResourceError::Capacity));
        drop(held);
        assert_eq!(bytes.used(), 0);
        let replacement = try_replacement().unwrap();
        assert_ne!(replacement.generation(), generation);
        drop(replacement);
        assert_eq!(
            resources
                .delivery()
                .snapshot()
                .endpoint_active_subscriptions,
            0
        );
    }
}

#[test]
fn client_complete_discards_only_matching_wholly_unsent_ids_in_the_shared_queue() {
    let resources = owner(2);
    let bytes = resources.delivery().byte_owner();
    let mut queue = WebSocketControlStaging::new(bytes.clone());
    queue.enqueue_error("same", &error_response("old")).unwrap();
    queue
        .enqueue_static(SubscriptionStaticControl::Heartbeat)
        .unwrap();
    queue.enqueue_complete("other").unwrap();
    queue.discard_operation("same").unwrap();
    assert_eq!(queue.queued_messages(), 2);
    assert_eq!(bytes.used(), queue.queued_bytes());
    let other = queue
        .begin_send_for(WebSocketControlClass::Priority)
        .unwrap()
        .unwrap();
    assert_eq!(other.websocket_operation_id(), Some("other"));
    assert_eq!(other.websocket_generation(), None);
    drop(other);
    queue.enqueue_error("same", &error_response("new")).unwrap();
    let current = queue
        .begin_send_for(WebSocketControlClass::Priority)
        .unwrap()
        .unwrap();
    assert!(
        std::str::from_utf8(current.as_bytes())
            .unwrap()
            .contains("new")
    );
    queue.discard_operation("same").unwrap();
    assert_eq!(queue.queued_messages(), 1);
    drop(current);
    queue.cancel();
    assert_eq!(bytes.used(), 0);
}

#[test]
fn invalid_owned_id_is_rejected_before_copy_or_queue_allocation() {
    for id in [String::new(), "x".repeat(257)] {
        let resources = owner(1);
        let bytes = resources.delivery().byte_owner();
        let mut queue = WebSocketControlStaging::new(bytes.clone());
        assert_eq!(
            queue.enqueue_complete(&id),
            Err(SubscriptionControlSignal::InvalidTransportArguments)
        );
        assert_eq!(queue.queued_messages(), 0);
        assert_eq!(bytes.used(), 0);
    }
}

#[test]
fn pending_rejection_keeps_its_exact_generation_without_inventing_active_quota() {
    let resources = owner(1);
    let bytes = resources.delivery().byte_owner();
    let mut queue = WebSocketControlStaging::new(bytes.clone());
    let connection = resources.open_connection().unwrap();
    let pending = connection.begin(Arc::from("pending")).unwrap();
    let generation = pending.generation();
    queue
        .enqueue_pending_error(Arc::from("pending"), generation, &error_response("invalid"))
        .unwrap();
    assert_eq!(resources.delivery().snapshot().endpoint_pending, 1);
    assert_eq!(
        resources
            .delivery()
            .snapshot()
            .endpoint_active_subscriptions,
        0
    );
    drop(pending.revoke().unwrap());
    assert_eq!(resources.delivery().snapshot().endpoint_pending, 0);
    let frame = queue
        .begin_send_for(WebSocketControlClass::Priority)
        .unwrap()
        .unwrap();
    assert_eq!(frame.websocket_generation(), Some(generation));
    assert_eq!(frame.websocket_operation_id(), Some("pending"));
    assert_eq!(
        resources
            .delivery()
            .snapshot()
            .endpoint_active_subscriptions,
        0
    );
    drop(frame);
    assert_eq!(bytes.used(), 0);
}
