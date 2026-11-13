//! ADR-0055 ordered endpoint authorization around the real sole socket writer.

use std::sync::Arc;
use std::time::Duration;

use super::super::state::{SourceStep, State, count};
use super::support::{Fixture, SUBSCRIPTION, messages_for};
use crate::managed::{Control, StopPhase};
use crate::{ShutdownOptions, ShutdownOutcome};

fn managed(fixture: &Fixture) -> Arc<Control> {
    let control = Control::new();
    control.activation_started().unwrap();
    fixture
        .driver
        .as_ref()
        .unwrap()
        .bind_managed_for_test(&control);
    control
}

fn stop(control: Arc<Control>, mode: usize) -> Box<dyn FnOnce() + Send> {
    Box::new(move || {
        let now = tokio::time::Instant::now();
        let (options, accepted) = match mode {
            0 => (ShutdownOptions::immediate(), now),
            1 => (ShutdownOptions::graceful(), now),
            2 => (ShutdownOptions::graceful(), now - Duration::from_secs(31)),
            _ => unreachable!(),
        };
        control.begin(options, accepted);
    })
}

fn queued(fixture: &mut Fixture, id: &str) {
    fixture.socket.0.lock().unwrap().ready = false;
    fixture.until(|driver| {
        driver
            .operation_state_for_test(id)
            .is_some_and(|(_, _, queued)| queued)
    });
}

#[test]
fn signal_after_event_preparation_reclaims_unstarted_progress_without_sending_backlog() {
    for mode in 0..3 {
        let probe = State::new(vec![
            SourceStep::Event(1),
            SourceStep::Event(2),
            SourceStep::Pending,
        ]);
        let mut fixture = Fixture::new(probe.clone());
        let control = managed(&fixture);
        fixture.socket.subscribe("waiting", SUBSCRIPTION);
        queued(&mut fixture, "waiting");
        fixture.until_source_pending(&probe);
        assert_eq!(count(&probe.counts.nested_calls), 2);
        assert_eq!(probe.event_generations.lock().unwrap().len(), 2);
        fixture
            .driver
            .as_mut()
            .unwrap()
            .before_send_for_test(stop(control.clone(), mode));
        fixture.socket.0.lock().unwrap().ready = true;
        fixture.until_closed();
        assert!(messages_for(&fixture.socket, "waiting").is_empty());
        assert_eq!(count(&probe.counts.source_drops), 1);
        assert_eq!(fixture.driver.as_ref().unwrap().encoded_bytes_for_test(), 0);
        if mode == 2 {
            assert_eq!(control.outcome(), Some(ShutdownOutcome::Expired));
        }
        fixture.finish();
    }
}

#[test]
fn authorization_before_signal_preserves_the_issued_frame_and_skips_later_results() {
    let probe = State::new(vec![
        SourceStep::Event(1),
        SourceStep::Event(2),
        SourceStep::Pending,
    ]);
    let mut fixture = Fixture::new(probe.clone());
    let control = managed(&fixture);
    fixture.socket.subscribe("issued", SUBSCRIPTION);
    queued(&mut fixture, "issued");
    fixture.until_source_pending(&probe);
    assert_eq!(count(&probe.counts.nested_calls), 2);
    assert_eq!(probe.event_generations.lock().unwrap().len(), 2);
    fixture.socket.0.lock().unwrap().on_start = Some(stop(control.clone(), 1));
    fixture.socket.0.lock().unwrap().ready = true;
    fixture.until_closed();
    let messages = messages_for(&fixture.socket, "issued");
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("\"value\":1"));
    assert!(messages[0].contains("\"droppedTotal\":0"));
    assert_eq!(count(&probe.counts.source_drops), 1);
    fixture.finish();
}

#[test]
fn signal_after_control_preparation_prevents_native_start() {
    for mode in 0..3 {
        let mut fixture = Fixture::new(State::new(Vec::new()));
        let control = managed(&fixture);
        fixture.socket.text(r#"{"type":"ping"}"#);
        fixture
            .driver
            .as_mut()
            .unwrap()
            .before_send_for_test(stop(control.clone(), mode));
        fixture.until_closed();
        assert_eq!(fixture.socket.messages(), [r#"{"type":"connection_ack"}"#]);
        fixture.finish();
    }
}

#[test]
fn signal_during_native_receive_rejects_subscribe_before_application_setup() {
    for mode in 0..3 {
        let probe = State::new(Vec::new());
        let mut fixture = Fixture::new(probe.clone());
        let control = managed(&fixture);
        let requests = count(&probe.counts.request_ids);
        fixture.socket.0.lock().unwrap().on_receive = Some(stop(control.clone(), mode));
        fixture.socket.subscribe("late", SUBSCRIPTION);
        fixture.until_closed();
        assert!(messages_for(&fixture.socket, "late").is_empty());
        assert_eq!(count(&probe.counts.factory_calls), 0);
        assert_eq!(count(&probe.counts.setup_calls), 0);
        assert_eq!(count(&probe.counts.source_polls), 0);
        assert_eq!(count(&probe.counts.request_ids), requests);
        fixture.finish();
    }
}

#[test]
fn graceful_signal_during_preparation_preserves_an_eligible_finite_response() {
    for operation in ["query { ping }", "mutation { ping }"] {
        let probe = State::new(Vec::new());
        let mut fixture = Fixture::new(probe.clone());
        let control = managed(&fixture);
        fixture.socket.subscribe("finite", operation);
        queued(&mut fixture, "finite");
        fixture
            .driver
            .as_mut()
            .unwrap()
            .before_send_for_test(stop(control.clone(), 1));
        fixture.socket.0.lock().unwrap().ready = true;
        fixture.until_closed();
        let messages = messages_for(&fixture.socket, "finite");
        assert_eq!(messages.len(), 2, "{messages:?}");
        assert!(messages[0].contains("\"data\":{\"ping\":\"pong\"}"));
        assert!(messages[1].contains("\"type\":\"complete\""));
        assert!(matches!(control.phase(), StopPhase::Graceful { .. }));
        assert_eq!(count(&probe.counts.factory_calls), 1);
        assert_eq!(count(&probe.counts.completion_drops), 1);
        fixture.finish();
    }
}
