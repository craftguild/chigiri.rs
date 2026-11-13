//! Changed common-session drain behavior with a bounded controllable socket.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::AtomicUsize;
use std::task::{Context, Waker};
use std::time::Duration;

use super::super::harness::event_gate;
use super::super::state::{SourceStep, State, count};
use super::support::{Fixture, SUBSCRIPTION, messages_for};
use crate::__streaming::WebSocketWriteProgress;

fn progress_until(fixture: &mut Fixture, counter: &AtomicUsize, expected: usize) {
    let driver = fixture.driver.as_mut().unwrap();
    fixture.runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(2), async {
            while count(counter) != expected {
                assert!(
                    Pin::new(&mut *driver)
                        .poll(&mut Context::from_waker(Waker::noop()))
                        .is_pending()
                );
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or_else(|error| {
            panic!(
                "socket fixture progress timed out: expected={expected}, actual={}, error={error}",
                count(counter)
            )
        });
    });
}

#[test]
fn graceful_socket_stops_pending_demand_and_does_not_accept_new_operations() {
    let probe = State::new(vec![SourceStep::Pending]);
    let mut fixture = Fixture::new(probe.clone());
    fixture.socket.subscribe("idle", SUBSCRIPTION);
    // Pending is stable across repeated worker polls; an exact poll count is not.
    fixture.until_source_pending_within(&probe, Duration::from_secs(2));
    fixture.driver.as_mut().unwrap().graceful();
    fixture.socket.subscribe("late", "query { ping }");
    fixture.until_closed();
    assert_eq!(count(&probe.counts.source_drops), 1);
    assert_eq!(count(&probe.counts.nested_calls), 0);
    assert!(messages_for(&fixture.socket, "late").is_empty());
    assert!(messages_for(&fixture.socket, "idle").is_empty());
    fixture.finish();
}

#[test]
fn graceful_socket_finishes_eligible_event_then_stops_before_the_next_source_item() {
    let probe = State::new(vec![SourceStep::Event(7), SourceStep::Event(8)]);
    let (endpoint, gate) = event_gate::endpoint(probe.clone());
    let mut fixture = Fixture::with_endpoint(endpoint);
    fixture.socket.subscribe("active", SUBSCRIPTION);
    progress_until(&mut fixture, &gate.entered, 1);
    fixture.driver.as_mut().unwrap().graceful();
    assert!(fixture.poll_once().is_pending());
    gate.release();
    fixture.until_closed();
    let messages = messages_for(&fixture.socket, "active");
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("\"value\":7"));
    assert_eq!(count(&probe.counts.source_polls), 1);
    assert_eq!(count(&probe.counts.nested_calls), 1);
    assert_eq!(count(&probe.counts.source_drops), 1);
    fixture.finish();
}

#[test]
fn graceful_socket_discards_queued_backlog_preserving_partial_frame_bytes_and_order() {
    let probe = State::new(vec![
        SourceStep::Event(1),
        SourceStep::Event(2),
        SourceStep::Event(3),
        SourceStep::Pending,
    ]);
    let mut fixture = Fixture::new(probe.clone());
    fixture.socket.0.lock().unwrap().automatic_flush = false;
    fixture.socket.subscribe("partial", SUBSCRIPTION);
    fixture.until_source_pending_within(&probe, Duration::from_secs(2));
    assert!(fixture.poll_once().is_pending());
    let immutable = fixture
        .socket
        .0
        .lock()
        .unwrap()
        .current
        .as_ref()
        .unwrap()
        .as_bytes()
        .to_vec();
    assert!(
        std::str::from_utf8(&immutable)
            .unwrap()
            .contains("\"value\":1")
    );
    fixture.driver.as_mut().unwrap().graceful();
    assert!(fixture.poll_once().is_pending());
    assert_eq!(
        fixture
            .socket
            .0
            .lock()
            .unwrap()
            .current
            .as_ref()
            .unwrap()
            .as_bytes(),
        immutable
    );
    fixture
        .socket
        .0
        .lock()
        .unwrap()
        .progress
        .push_back(WebSocketWriteProgress {
            accepted: 1,
            flushed: false,
        });
    assert!(fixture.poll_once().is_pending());
    assert_eq!(
        fixture
            .socket
            .0
            .lock()
            .unwrap()
            .current
            .as_ref()
            .unwrap()
            .as_bytes(),
        immutable
    );
    fixture
        .socket
        .0
        .lock()
        .unwrap()
        .progress
        .push_back(WebSocketWriteProgress {
            accepted: immutable.len() - 1,
            flushed: true,
        });
    fixture.until_closed();
    let messages = messages_for(&fixture.socket, "partial");
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("\"droppedTotal\":0"));
    assert_eq!(count(&probe.counts.nested_calls), 3);
    assert_eq!(count(&probe.counts.source_drops), 1);
    fixture.finish();
}

#[test]
fn graceful_socket_finishes_inflight_query_and_mutation_context_execution() {
    for operation in ["query { ping }", "mutation { ping }"] {
        let probe = State::new(Vec::new());
        let (endpoint, gate) = event_gate::finite_endpoint(probe.clone());
        let mut fixture = Fixture::with_endpoint(endpoint);
        fixture.socket.subscribe("finite", operation);
        progress_until(&mut fixture, &gate.entered, 1);
        fixture.driver.as_mut().unwrap().graceful();
        assert!(fixture.poll_once().is_pending());
        gate.release();
        fixture.until_closed();
        let messages = messages_for(&fixture.socket, "finite");
        assert_eq!(messages.len(), 2, "{messages:?}");
        assert!(messages[0].contains("\"data\":{\"ping\":\"pong\"}"));
        assert!(messages[1].contains("\"type\":\"complete\""));
        assert_eq!(count(&probe.counts.factory_calls), 1);
        assert_eq!(count(&probe.counts.completion_drops), 1);
        fixture.finish();
    }
}
