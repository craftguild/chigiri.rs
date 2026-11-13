use std::sync::Arc;

use chigiri_http::SubscriptionStaticControl;

use super::super::{SubscriptionControlSignal, WebSocketControlClass as Class};
use super::fixture::{bytes, error_response, heartbeat_control, peer_payload, websocket};

#[test]
fn priority_bypasses_only_ordinary_controls_without_eviction_or_a_second_writer() {
    let owner = bytes(1024 * 1024);
    let mut staging = websocket(&owner);
    staging.enqueue_static(heartbeat_control()).unwrap();
    staging
        .enqueue_static(SubscriptionStaticControl::ConnectionAck)
        .unwrap();
    staging
        .enqueue_peer_pong(&peer_payload(r#"{"type":"ping","payload":{"x":1}}"#))
        .unwrap();
    staging
        .enqueue_error("first", &error_response("safe"))
        .unwrap();
    staging.enqueue_complete("second").unwrap();
    let total = staging.queued_bytes();
    assert_eq!(owner.used(), total);
    assert_eq!(staging.queued_messages(), 5);

    let ack = Arc::new(staging.begin_send_for(Class::Priority).unwrap().unwrap());
    assert_eq!(ack.as_bytes(), br#"{"type":"connection_ack"}"#);
    assert_eq!(owner.used(), total);
    assert_eq!(staging.queued_messages(), 4);
    assert!(staging.begin_send_for(Class::Ordinary).unwrap().is_none());
    let held = Arc::clone(&ack);
    drop(ack);
    assert!(staging.begin_send_for(Class::Priority).unwrap().is_none());
    drop(held);

    for expected in ["error", "complete"] {
        let frame = staging.begin_send_for(Class::Priority).unwrap().unwrap();
        assert!(
            std::str::from_utf8(frame.as_bytes())
                .unwrap()
                .contains(&format!("\"type\":\"{expected}\""))
        );
        assert_eq!(owner.used(), frame.len() + staging.queued_bytes());
        drop(frame);
    }
    assert!(!staging.has_queued(Class::Priority));
    assert!(staging.has_queued(Class::Ordinary));
    for expected in ["ping", "pong"] {
        let frame = staging.begin_send_for(Class::Ordinary).unwrap().unwrap();
        assert!(
            std::str::from_utf8(frame.as_bytes())
                .unwrap()
                .contains(&format!("\"type\":\"{expected}\""))
        );
        assert_eq!(owner.used(), frame.len() + staging.queued_bytes());
        drop(frame);
    }
    assert_eq!(owner.used(), 0);
    assert_eq!(staging.queued_messages(), 0);
}

#[test]
fn original_fifo_entrypoint_and_cancelled_physical_ownership_are_preserved() {
    let owner = bytes(1024 * 1024);
    let mut staging = websocket(&owner);
    staging.enqueue_static(heartbeat_control()).unwrap();
    assert!(staging.begin_send_for(Class::Priority).unwrap().is_none());
    staging
        .enqueue_static(SubscriptionStaticControl::ConnectionAck)
        .unwrap();
    let ping = staging.begin_send().unwrap().unwrap();
    assert_eq!(ping.as_bytes(), br#"{"type":"ping"}"#);
    staging.cancel();
    assert_eq!(owner.used(), ping.len());
    assert_eq!(staging.queued_messages(), 0);
    assert!(!staging.has_queued(Class::Priority));
    assert!(matches!(
        staging.begin_send_for(Class::Priority),
        Err(SubscriptionControlSignal::Cancelled)
    ));
    drop(ping);
    assert_eq!(owner.used(), 0);
}
