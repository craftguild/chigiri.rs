use std::future::poll_fn;
use std::sync::Arc;

use chigiri_http::SubscriptionTransport;
use chigiri_runtime::SubscriptionOverflow;

use super::super::harness;
use super::super::state::State;
use super::support::{NEVER_DEADLINES, Registered, echo_request, run, worker};
use crate::subscription::bridge_resources::{BridgeCleanupOutcome, BridgeResourceError};
use crate::subscription::outbox::SubscriptionOutbox;
use crate::subscription::worker::websocket::WebSocketValidationOutcome;

#[test]
fn invalid_kind_revokes_pending_generation_without_starting_a_worker() {
    let probe = State::new(Vec::new());
    let registered = Registered::new(probe);
    let id: Arc<str> = Arc::from("invalid-one-shot-kind");
    let validation = worker(
        &registered.runtime,
        &registered.endpoint,
        harness::request(),
        None,
        registered.begin(Arc::clone(&id)),
        &NEVER_DEADLINES,
    );
    let WebSocketValidationOutcome::Prepared(prepared) = run(&registered.runtime, validation)
    else {
        panic!("subscription validation did not prepare")
    };
    let mut outbox = outbox(&registered, Arc::clone(&id));
    let failure = match prepared.activate_one_shot(outbox.reserve_before_poll(0).unwrap()) {
        Ok(_) => panic!("subscription unexpectedly activated as a one-shot"),
        Err(failure) => failure,
    };
    assert_eq!(failure.error(), BridgeResourceError::InvalidOperationKind);
    let mut terminal = failure.into_terminal().unwrap();
    let replacement = registered.connection.begin(id).unwrap();
    assert_ne!(replacement.generation(), terminal.generation());
    assert_eq!(
        run(
            &registered.runtime,
            poll_fn(|context| terminal.poll_cleanup(context))
        ),
        BridgeCleanupOutcome::Completed
    );
    drop((terminal, replacement, outbox));
    registered.drain();
}

#[test]
fn active_spawn_failure_revokes_generation_and_retains_physical_owner() {
    let probe = State::new(Vec::new());
    let registered = Registered::new(probe);
    let id: Arc<str> = Arc::from("active-spawn-failure");
    let validation = worker(
        &registered.runtime,
        &registered.endpoint,
        echo_request(),
        None,
        registered.begin(Arc::clone(&id)),
        &NEVER_DEADLINES,
    );
    let WebSocketValidationOutcome::Prepared(prepared) = run(&registered.runtime, validation)
    else {
        panic!("query validation did not prepare")
    };
    let mut outbox = outbox(&registered, Arc::clone(&id));
    let failure = match prepared.activate_one_shot(outbox.reserve_before_poll(0).unwrap()) {
        Ok(_) => panic!("one-shot worker unexpectedly started outside a runtime"),
        Err(failure) => failure,
    };
    assert_eq!(failure.error(), BridgeResourceError::Invariant);
    let mut terminal = failure.into_terminal().unwrap();
    let physical = terminal.take_physical().unwrap();
    assert_eq!(physical.generation(), terminal.generation());
    let replacement = registered.connection.begin(id).unwrap();
    assert_ne!(replacement.generation(), terminal.generation());
    assert_eq!(
        run(
            &registered.runtime,
            poll_fn(|context| terminal.poll_cleanup(context))
        ),
        BridgeCleanupOutcome::Completed
    );
    drop((physical, terminal, replacement, outbox));
    registered.drain();
}

fn outbox(registered: &Registered, id: Arc<str>) -> SubscriptionOutbox {
    SubscriptionOutbox::new(
        &registered.endpoint.subscriptions.settings,
        registered
            .endpoint
            .subscriptions
            .resources
            .delivery()
            .byte_owner(),
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::WebSocket,
        Some(id),
    )
    .unwrap()
}
