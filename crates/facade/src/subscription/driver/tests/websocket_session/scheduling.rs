//! Real root arbitration keeps priority and sparse whole-event fairness.

use super::super::state::{SourceStep, State};
use super::support::{Fixture, SUBSCRIPTION, kind, messages_for};

#[test]
fn eight_ordinary_controls_then_round_robin_events_share_the_same_writer() {
    let mut fixture = Fixture::new(State::new(vec![SourceStep::Event(7), SourceStep::Event(8)]));
    fixture.socket.0.lock().unwrap().ready = false;
    for id in ["a", "b"] {
        fixture.socket.subscribe(id, SUBSCRIPTION);
    }
    fixture.until(|driver| {
        ["a", "b"].iter().all(|id| {
            driver
                .operation_state_for_test(id)
                .is_some_and(|(_, live, queued)| !live && queued)
        })
    });
    for _ in 0..9 {
        fixture.socket.text(r#"{"type":"ping"}"#);
    }
    assert!(fixture.poll_once().is_pending());
    fixture.socket.0.lock().unwrap().ready = true;
    let handle = fixture.socket.clone();
    fixture
        .until(|_| messages_for(&handle, "a").len() == 3 && messages_for(&handle, "b").len() == 3);
    let messages = fixture.socket.messages();
    let first_event = messages
        .iter()
        .position(|message| kind(message) == "next")
        .unwrap();
    assert_eq!(
        messages[1..first_event]
            .iter()
            .filter(|message| kind(message) == "pong")
            .count(),
        8
    );
    let event_ids: Vec<_> = messages
        .iter()
        .filter(|message| kind(message) == "next")
        .map(|message| {
            if message.contains(r#""id":"a""#) {
                "a"
            } else {
                "b"
            }
        })
        .collect();
    assert_eq!(event_ids, ["a", "b", "a", "b"]);
    assert_eq!(
        messages
            .iter()
            .filter(|message| kind(message) == "pong")
            .count(),
        9
    );
    fixture.finish();
}

#[test]
fn operation_error_priority_preempts_queued_ordinary_controls() {
    let mut fixture = Fixture::new(State::new(Vec::new()));
    fixture.socket.0.lock().unwrap().ready = false;
    for _ in 0..9 {
        fixture.socket.text(r#"{"type":"ping"}"#);
    }
    fixture.socket.subscribe("invalid", "query { missing }");
    fixture.until(|driver| driver.queued_priority_for_test());
    fixture.socket.0.lock().unwrap().ready = true;
    let handle = fixture.socket.clone();
    fixture.until(|_| handle.messages().len() == 11);
    let messages = fixture.socket.messages();
    assert_eq!(kind(&messages[1]), "error");
    assert_eq!(
        messages[2..]
            .iter()
            .filter(|message| kind(message) == "pong")
            .count(),
        9
    );
    fixture.finish();
}
