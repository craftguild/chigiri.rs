//! The real ingress path keeps one setup budget before and after validation.

use std::sync::Arc;
use std::time::Duration;

use super::super::state::{State, count};
use super::support::{Fixture, kind, messages_for};
use crate::SubscriptionLimits;

const SETUP: Duration = Duration::from_secs(7);
const ECHO: &str = r#"{"id":"setup","type":"subscribe","payload":{"query":"query Named($value: Probe!) { echo(value: $value) }","operationName":"Named","variables":{"value":"worker-value"}}}"#;

#[test]
fn original_setup_timer_survives_ingress_validation_without_restart() {
    let probe = State::new(Vec::new());
    let gate = probe.block_scalar();
    let limits = SubscriptionLimits {
        setup_timeout: SETUP,
        ..SubscriptionLimits::default()
    };
    let mut fixture = Fixture::with_limits(Arc::clone(&probe), limits);
    fixture.socket.text(ECHO);
    assert!(fixture.poll_once().is_pending());
    assert!(gate.wait_entered(Duration::from_secs(2)));
    assert_eq!(fixture.clock.starts_for(SETUP), [0]);
    assert_eq!(count(&probe.counts.factory_calls), 0);
    fixture.advance(Duration::from_secs(6));
    gate.release();
    let handle = fixture.socket.clone();
    fixture.until(|_| messages_for(&handle, "setup").len() == 2);
    let messages = messages_for(&fixture.socket, "setup");
    assert!(messages[0].contains(r#""data":{"echo":"worker-value"}"#));
    assert_eq!(kind(&messages[1]), "complete");
    assert_eq!(
        fixture.clock.starts_for(SETUP),
        [0],
        "handoff must not recreate the original budget"
    );
    assert_eq!(count(&probe.counts.scalar_calls), 1);
    assert_eq!(count(&probe.counts.factory_calls), 1);
    fixture.finish();
}

#[test]
fn original_setup_expiry_releases_id_while_blocked_validation_still_owns_cleanup() {
    let probe = State::new(Vec::new());
    let gate = probe.block_scalar();
    let limits = SubscriptionLimits {
        setup_timeout: SETUP,
        ..SubscriptionLimits::default()
    };
    let mut fixture = Fixture::with_limits(Arc::clone(&probe), limits);
    fixture.socket.text(ECHO);
    assert!(fixture.poll_once().is_pending());
    assert!(gate.wait_entered(Duration::from_secs(2)));
    fixture.advance(SETUP);
    let handle = fixture.socket.clone();
    fixture.until(|_| messages_for(&handle, "setup").len() == 1);
    let messages = messages_for(&fixture.socket, "setup");
    assert_eq!(kind(&messages[0]), "error");
    assert!(messages[0].contains(r#""code":"REQUEST_TIMEOUT""#));
    assert_eq!(count(&probe.counts.factory_calls), 0);
    // The original scalar callback has not returned or dropped. A replacement
    // must be admitted now rather than treated as a duplicate pending ID.
    fixture.socket.subscribe("setup", "query { ping }");
    let handle = fixture.socket.clone();
    fixture.until(|_| messages_for(&handle, "setup").len() == 3);
    assert!(messages_for(&fixture.socket, "setup")[1].contains(r#""data":{"ping":"pong"}"#));
    assert!(fixture.socket.0.lock().unwrap().closes.is_empty());
    gate.release();
    fixture.finish();
}
