//! Admission and client-complete cutoff on the actual root writer timeline.

use super::super::state::{SourceStep, State};
use super::support::{Fixture, SUBSCRIPTION, kind, messages_for};
use crate::__streaming::WebSocketWriteProgress;

#[test]
fn accepted_same_id_suppresses_old_unsent_events_and_completion() {
    let probe = State::new(vec![SourceStep::Event(7), SourceStep::Event(8)]);
    let mut fixture = Fixture::new(probe);
    fixture.socket.0.lock().unwrap().ready = false;
    fixture.socket.subscribe("reuse", SUBSCRIPTION);
    fixture.until(|driver| {
        driver
            .operation_state_for_test("reuse")
            .is_some_and(|(_, live, queued)| !live && queued)
    });
    let old = fixture
        .driver
        .as_ref()
        .unwrap()
        .operation_state_for_test("reuse")
        .unwrap()
        .0;
    fixture.socket.subscribe("reuse", "query { ping }");
    fixture.until(|driver| {
        driver
            .operation_state_for_test("reuse")
            .is_some_and(|(generation, _, queued)| generation != old && queued)
    });
    fixture.socket.0.lock().unwrap().ready = true;
    let handle = fixture.socket.clone();
    fixture.until(|_| messages_for(&handle, "reuse").len() == 2);
    let messages = messages_for(&fixture.socket, "reuse");
    assert!(messages[0].contains(r#""data":{"ping":"pong"}"#));
    assert_eq!(kind(&messages[1]), "complete");
    assert!(fixture.socket.0.lock().unwrap().closes.is_empty());
    fixture.finish();
}

#[test]
fn client_complete_suppresses_every_old_unsent_event_and_terminal() {
    let mut fixture = Fixture::new(State::new(vec![SourceStep::Event(7), SourceStep::Event(8)]));
    fixture.socket.0.lock().unwrap().ready = false;
    fixture.socket.subscribe("cancel", SUBSCRIPTION);
    fixture.until(|driver| {
        driver
            .operation_state_for_test("cancel")
            .is_some_and(|(_, live, queued)| !live && queued)
    });
    fixture.socket.text(r#"{"id":"cancel","type":"complete"}"#);
    fixture.until(|driver| driver.operation_state_for_test("cancel").is_none());
    fixture.socket.0.lock().unwrap().ready = true;
    fixture.socket.subscribe("peer", "query { ping }");
    let handle = fixture.socket.clone();
    fixture.until(|_| messages_for(&handle, "peer").len() == 2);
    assert!(messages_for(&fixture.socket, "cancel").is_empty());
    fixture.finish();
}

#[test]
fn old_started_partial_event_finishes_but_its_queued_successor_is_invalidated() {
    let mut fixture = Fixture::new(State::new(vec![SourceStep::Event(7), SourceStep::Event(8)]));
    fixture.socket.0.lock().unwrap().automatic_flush = false;
    fixture.socket.subscribe("reuse", SUBSCRIPTION);
    let handle = fixture.socket.clone();
    fixture.until(|driver| {
        driver
            .operation_state_for_test("reuse")
            .is_some_and(|(_, live, _)| !live)
            && !messages_for(&handle, "reuse").is_empty()
    });
    let length = fixture.socket.current_len();
    let old = fixture
        .driver
        .as_ref()
        .unwrap()
        .operation_state_for_test("reuse")
        .unwrap()
        .0;
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
    fixture.socket.subscribe("reuse", "query { ping }");
    fixture.until(|driver| {
        driver
            .operation_state_for_test("reuse")
            .is_some_and(|(generation, _, queued)| generation != old && queued)
    });
    assert_eq!(messages_for(&fixture.socket, "reuse").len(), 1);
    {
        let mut state = fixture.socket.0.lock().unwrap();
        state.progress.push_back(WebSocketWriteProgress {
            accepted: length - 1,
            flushed: true,
        });
        state.automatic_flush = true;
    }
    let handle = fixture.socket.clone();
    fixture.until(|_| messages_for(&handle, "reuse").len() == 3);
    let messages = messages_for(&fixture.socket, "reuse");
    assert!(messages[0].contains(r#""data":{"chosen":{"value":7}}"#));
    assert!(messages[1].contains(r#""data":{"ping":"pong"}"#));
    assert_eq!(kind(&messages[2]), "complete");
    fixture.finish();
}

#[test]
fn replacement_invalidates_terminal_permission_after_its_slot_was_reaped() {
    for query in [SUBSCRIPTION, "query { missing }"] {
        let mut fixture = Fixture::new(State::new(Vec::new()));
        fixture.socket.0.lock().unwrap().automatic_flush = false;
        fixture.socket.subscribe("blocker", "query { ping }");
        let handle = fixture.socket.clone();
        fixture.until(|_| messages_for(&handle, "blocker").len() == 1);
        fixture.socket.subscribe("reuse", query);
        fixture.until(|driver| {
            driver.operation_state_for_test("reuse").is_none() && driver.queued_priority_for_test()
        });
        fixture.socket.subscribe("reuse", "query { ping }");
        fixture.until(|driver| driver.operation_state_for_test("reuse").is_some());
        fixture.socket.0.lock().unwrap().automatic_flush = true;
        fixture.socket.0.lock().unwrap().ready = true;
        let handle = fixture.socket.clone();
        fixture.until(|_| messages_for(&handle, "reuse").len() == 2);
        let messages = messages_for(&fixture.socket, "reuse");
        assert!(messages.iter().all(|message| kind(message) != "error"));
        fixture.finish();
    }
}
