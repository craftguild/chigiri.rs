//! Actual public session Future tests, separate from live official-port traffic.

mod admission;
mod deadlines;
mod fences;
mod graceful;
mod observation;
mod physical;
mod reuse;
mod scheduling;
mod setup_fences;
pub(in crate::subscription::driver::tests) mod socket;
mod support;
mod timers;

use std::sync::Arc;

use chigiri_http::{RequestPhase, SubscriptionTransport, WebSocketOneShot};

use super::state::{SourceStep, State, count};
use support::{Fixture, SUBSCRIPTION, delivery_value, kind, messages_for};

#[test]
fn same_socket_query_mutation_and_subscription_use_concrete_fresh_execution() {
    let probe = State::new(vec![SourceStep::Event(7), SourceStep::Event(8)]);
    let mut fixture = Fixture::new(Arc::clone(&probe));
    assert_eq!(count(&probe.counts.factory_calls), 0);
    assert_eq!(fixture.socket.messages(), [r#"{"type":"connection_ack"}"#]);
    fixture.socket.subscribe("query", "query { ping }");
    fixture.socket.subscribe("mutation", "mutation { ping }");
    fixture.socket.subscribe("subscription", SUBSCRIPTION);
    let handle = fixture.socket.clone();
    fixture.until(|_| {
        messages_for(&handle, "subscription")
            .last()
            .is_some_and(|message| kind(message) == "complete")
            && messages_for(&handle, "query").len() == 2
            && messages_for(&handle, "mutation").len() == 2
    });
    for id in ["query", "mutation"] {
        let messages = messages_for(&fixture.socket, id);
        assert_eq!(kind(&messages[0]), "next");
        assert!(messages[0].contains(r#""data":{"ping":"pong"}"#));
        assert_eq!(delivery_value(&messages[0]), "null");
        assert_eq!(kind(&messages[1]), "complete");
    }
    let messages = messages_for(&fixture.socket, "subscription");
    assert_eq!(messages.len(), 3);
    for (index, value) in [7, 8].into_iter().enumerate() {
        assert!(messages[index].contains(&format!(r#""data":{{"chosen":{{"value":{value}}}}}"#)));
        assert_eq!(delivery_value(&messages[index]), "0");
    }
    assert_eq!(count(&probe.counts.setup_calls), 1);
    assert_eq!(count(&probe.counts.source_polls), 3);
    assert_eq!(count(&probe.counts.source_drops), 1);
    assert_eq!(count(&probe.counts.factory_calls), 5);
    assert_eq!(count(&probe.counts.hook_calls), 0);
    assert_eq!(count(&probe.counts.completion_calls), 0);
    assert_eq!(count(&probe.counts.completion_drops), 5);
    let phases = probe.phases.lock().unwrap().clone();
    for operation in [WebSocketOneShot::Query, WebSocketOneShot::Mutation] {
        assert!(phases.contains(&RequestPhase::WebSocketOperation { operation }));
    }
    assert!(phases.contains(&RequestPhase::SubscriptionSetup {
        transport: SubscriptionTransport::WebSocket
    }));
    assert_eq!(
        phases
            .iter()
            .filter(|phase| matches!(phase, RequestPhase::SubscriptionEvent { .. }))
            .count(),
        2
    );
    fixture.finish();
}

#[test]
fn validation_and_source_errors_do_not_complete_or_cancel_a_healthy_peer() {
    let probe = State::new(vec![SourceStep::Event(7), SourceStep::Error]);
    let mut fixture = Fixture::new(Arc::clone(&probe));
    fixture.socket.subscribe(
        "invalid",
        "subscription { chosen: events(limit: 2) { missing } }",
    );
    fixture.socket.subscribe("source", SUBSCRIPTION);
    fixture.socket.subscribe("peer", "query { ping }");
    let handle = fixture.socket.clone();
    fixture.until(|_| {
        messages_for(&handle, "source")
            .last()
            .is_some_and(|message| kind(message) == "error")
            && messages_for(&handle, "invalid").len() == 1
            && messages_for(&handle, "peer").len() == 2
    });
    for id in ["source", "invalid"] {
        assert!(
            !messages_for(&fixture.socket, id)
                .iter()
                .any(|message| kind(message) == "complete")
        );
    }
    let source = messages_for(&fixture.socket, "source");
    assert_eq!(source.len(), 2);
    assert!(source[0].contains(r#""data":{"chosen":{"value":7}}"#));
    assert_eq!(count(&probe.counts.source_maps), 1);
    assert_eq!(count(&probe.counts.setup_calls), 1);
    assert!(fixture.socket.0.lock().unwrap().closes.is_empty());
    fixture.finish();
}
