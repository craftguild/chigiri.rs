use chigiri_http::{
    __subscription_static_control_len, SubscriptionStaticControl, SubscriptionTransport,
};

use super::super::super::delivery::EndpointByteOwner;
use super::super::SubscriptionControlSignal;
use super::super::websocket::{
    MAX_QUEUED_CONTROL_BYTES, MAX_QUEUED_CONTROL_MESSAGES, QueueAccounting,
};
use super::fixture::{bytes, error_response, heartbeat_control, peer_payload, websocket};

#[test]
fn exact_queue_count_and_byte_boundaries_reject_one_over_without_eviction() {
    let mut count = QueueAccounting::default();
    for _ in 0..MAX_QUEUED_CONTROL_MESSAGES {
        count.admit(0).unwrap();
    }
    assert_eq!(
        count.admit(0),
        Err(SubscriptionControlSignal::QueueCapacity)
    );
    assert_eq!(count.messages, MAX_QUEUED_CONTROL_MESSAGES);

    let mut bytes = QueueAccounting::default();
    bytes.admit(MAX_QUEUED_CONTROL_BYTES).unwrap();
    assert_eq!(bytes.bytes, MAX_QUEUED_CONTROL_BYTES);
    assert_eq!(
        QueueAccounting::default().admit(MAX_QUEUED_CONTROL_BYTES + 1),
        Err(SubscriptionControlSignal::QueueCapacity)
    );
}

#[test]
fn fixed_count_fullness_preserves_every_existing_control() {
    let byte_owner = bytes(1024 * 1024);
    let mut staging = websocket(&byte_owner);
    let one =
        __subscription_static_control_len(SubscriptionTransport::WebSocket, heartbeat_control())
            .unwrap();
    for _ in 0..MAX_QUEUED_CONTROL_MESSAGES {
        staging.enqueue_static(heartbeat_control()).unwrap();
    }
    assert_eq!(staging.queued_messages(), MAX_QUEUED_CONTROL_MESSAGES);
    assert_eq!(staging.queued_bytes(), one * MAX_QUEUED_CONTROL_MESSAGES);
    assert_eq!(
        staging.enqueue_static(heartbeat_control()),
        Err(SubscriptionControlSignal::QueueCapacity)
    );
    assert_eq!(staging.queued_messages(), MAX_QUEUED_CONTROL_MESSAGES);
    assert_eq!(staging.queued_bytes(), one * MAX_QUEUED_CONTROL_MESSAGES);
    assert_eq!(byte_owner.used(), one * MAX_QUEUED_CONTROL_MESSAGES);
    staging.cancel();
    assert_eq!(byte_owner.used(), 0);
}

#[test]
fn static_and_peer_pong_use_exact_reservations_and_release_on_drop() {
    let byte_owner = bytes(1024 * 1024);
    let mut staging = websocket(&byte_owner);
    staging
        .enqueue_static(SubscriptionStaticControl::ConnectionAck)
        .unwrap();
    let payload = peer_payload(r#"{"type":"ping","payload":{"n":1e70000}}"#);
    staging.enqueue_peer_pong(&payload).unwrap();
    assert_eq!(byte_owner.used(), staging.queued_bytes());

    let first = staging.begin_send().unwrap().unwrap();
    assert_eq!(byte_owner.used(), first.len() + staging.queued_bytes());
    assert!(staging.begin_send().unwrap().is_none());
    drop(first);
    let second = staging.begin_send().unwrap().unwrap();
    assert_eq!(
        second.as_bytes(),
        br#"{"type":"pong","payload":{"n":1e70000}}"#
    );
    assert_eq!(byte_owner.used(), second.len());
    drop(second);
    assert_eq!(byte_owner.used(), 0);
}

#[test]
fn dynamic_errors_shrink_reservation_and_encoding_failure_is_terminal() {
    let byte_owner = bytes(128 * 1024);
    let mut staging = websocket(&byte_owner);
    staging
        .enqueue_error("operation", &error_response("bounded"))
        .unwrap();
    assert_eq!(byte_owner.used(), staging.queued_bytes());
    assert!(byte_owner.used() < 64 * 1024);
    staging.cancel();
    assert_eq!(byte_owner.used(), 0);

    let mut failed = websocket(&byte_owner);
    assert_eq!(
        failed.enqueue_error("operation", &error_response("x".repeat(70 * 1024))),
        Err(SubscriptionControlSignal::ControlLimit)
    );
    assert_eq!(byte_owner.used(), 0);
    assert_eq!(
        failed.enqueue_complete("operation"),
        Err(SubscriptionControlSignal::ControlLimit)
    );
}

#[test]
fn endpoint_pressure_is_shared_and_reserved_before_encoding() {
    let static_bytes =
        __subscription_static_control_len(SubscriptionTransport::WebSocket, heartbeat_control())
            .unwrap();
    let owner = EndpointByteOwner::new(static_bytes);
    let mut first = websocket(&owner);
    let mut second = websocket(&owner);
    first.enqueue_static(heartbeat_control()).unwrap();
    assert_eq!(
        second.enqueue_static(heartbeat_control()),
        Err(SubscriptionControlSignal::SharedByteCapacity)
    );
    assert_eq!(owner.used(), static_bytes);
    first.cancel();
    assert_eq!(owner.used(), 0);
}
