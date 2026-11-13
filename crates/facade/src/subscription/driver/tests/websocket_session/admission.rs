//! Whole-connection protocol errors versus isolated bounded admission failures.

use std::sync::Arc;

use super::super::state::{SourceStep, State, count};
use super::support::{Fixture, SUBSCRIPTION, kind, messages_for};
use crate::SubscriptionLimits;

#[test]
fn a_genuinely_live_duplicate_id_closes_4409_not_an_operation_error() {
    let probe = State::new(vec![SourceStep::Pending]);
    let mut fixture = Fixture::new(Arc::clone(&probe));
    fixture.socket.subscribe("duplicate", SUBSCRIPTION);
    fixture.until_source_pending(&probe);
    fixture.socket.subscribe("duplicate", "query { ping }");
    fixture.until_closed();
    assert_eq!(fixture.socket.0.lock().unwrap().closes, [Some(4409)]);
    assert!(messages_for(&fixture.socket, "duplicate").is_empty());
    fixture.finish();
    assert_eq!(count(&probe.counts.source_drops), 1);
}

#[test]
fn oversized_id_rejects_only_that_operation_before_any_context_or_source() {
    let probe = State::new(Vec::new());
    let mut fixture = Fixture::new(Arc::clone(&probe));
    let id = "x".repeat(257);
    fixture.socket.subscribe(&id, "query { ping }");
    fixture.socket.subscribe("peer", "query { ping }");
    let handle = fixture.socket.clone();
    fixture.until(|_| {
        messages_for(&handle, &id).len() == 1 && messages_for(&handle, "peer").len() == 2
    });
    assert_eq!(kind(&messages_for(&fixture.socket, &id)[0]), "error");
    assert_eq!(count(&probe.counts.factory_calls), 1);
    assert_eq!(count(&probe.counts.setup_calls), 0);
    assert!(fixture.socket.0.lock().unwrap().closes.is_empty());
    fixture.finish();
}

#[test]
fn subscription_capacity_failure_preserves_existing_source_and_one_shot_peer() {
    let probe = State::new(vec![SourceStep::Pending]);
    let limits = SubscriptionLimits {
        max_active_subscriptions: 1,
        ..SubscriptionLimits::default()
    };
    let mut fixture = Fixture::with_limits(Arc::clone(&probe), limits);
    fixture.socket.subscribe("existing", SUBSCRIPTION);
    fixture.until_source_pending(&probe);
    fixture.socket.subscribe("rejected", SUBSCRIPTION);
    fixture.socket.subscribe("peer", "query { ping }");
    let handle = fixture.socket.clone();
    fixture.until(|_| {
        messages_for(&handle, "rejected").len() == 1 && messages_for(&handle, "peer").len() == 2
    });
    assert_eq!(kind(&messages_for(&fixture.socket, "rejected")[0]), "error");
    assert!(messages_for(&fixture.socket, "rejected")[0].contains("SUBSCRIPTION_LIMIT_EXCEEDED"));
    assert!(messages_for(&fixture.socket, "existing").is_empty());
    assert_eq!(count(&probe.counts.setup_calls), 1);
    assert_eq!(count(&probe.counts.source_drops), 0);
    assert_eq!(count(&probe.counts.factory_calls), 2);
    assert!(fixture.socket.0.lock().unwrap().closes.is_empty());
    fixture.finish();
    assert_eq!(count(&probe.counts.source_drops), 1);
}

#[test]
fn client_complete_immediately_permits_same_id_while_source_cancellation_drains() {
    let probe = State::new(vec![SourceStep::Pending]);
    let mut fixture = Fixture::new(Arc::clone(&probe));
    fixture.socket.subscribe("reuse", SUBSCRIPTION);
    fixture.until_source_pending(&probe);
    fixture.socket.text(r#"{"id":"reuse","type":"complete"}"#);
    fixture.socket.subscribe("reuse", "query { ping }");
    let handle = fixture.socket.clone();
    fixture.until(|_| messages_for(&handle, "reuse").len() == 2);
    let messages = messages_for(&fixture.socket, "reuse");
    assert!(messages[0].contains(r#""data":{"ping":"pong"}"#));
    assert_eq!(kind(&messages[1]), "complete");
    assert!(fixture.socket.0.lock().unwrap().closes.is_empty());
    fixture.finish();
}
