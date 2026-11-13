use std::future::poll_fn;
use std::sync::Arc;
use std::task::{Context, Wake, Waker};

use chigiri_http::SubscriptionTransport;
use chigiri_runtime::SubscriptionOverflow;
use chigiri_runtime::{GraphQlError, GraphQlResponse};

use super::super::harness;
use super::super::state::{EventMode, FactoryMode, HookMode, SetupMode, SourceStep, State, count};
use super::support::{
    NEVER_DEADLINES, Registered, echo_request, run, setup_deadline, validation, worker,
};
use crate::subscription::bridge_resources::BridgeCleanupOutcome;
use crate::subscription::failure::SubscriptionFailureReporter;
use crate::subscription::outbox::SubscriptionOutbox;
use crate::subscription::worker::websocket::{
    WebSocketOneShotOutcome, WebSocketSubscriptionSetupOutcome, WebSocketTerminalMessage,
    WebSocketValidationOutcome, WebSocketValidationWorker,
};

struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

#[test]
fn validation_start_failure_has_no_worker_and_retains_deadline_until_cleanup() {
    let probe = State::new(Vec::new());
    let registered = Registered::new(Arc::clone(&probe));
    let id: Arc<str> = Arc::from("start-failure");
    let operation = registered.begin(Arc::clone(&id));
    let failure = WebSocketValidationWorker::new(
        validation(&registered.endpoint, echo_request(), None),
        operation,
        setup_deadline(&NEVER_DEADLINES),
        SubscriptionFailureReporter::new(
            registered.endpoint.subscription_execution.schema.clone(),
            registered.endpoint.schema.request_telemetry(None),
            "subscription-websocket-worker-test",
        ),
    )
    .err()
    .expect("starting outside a Tokio runtime must return every owner");
    assert!(failure.retains_original_deadline());

    let mut terminal = failure.into_terminal().unwrap();
    let generation = terminal.generation();
    let replacement = registered.connection.begin(id).unwrap();
    assert_ne!(replacement.generation(), generation);
    assert_eq!(
        run(
            &registered.runtime,
            poll_fn(|context| terminal.poll_cleanup(context))
        ),
        BridgeCleanupOutcome::Completed
    );
    drop(terminal);
    drop(replacement);
    registered.drain();
}

#[test]
fn prepared_rejection_retains_the_original_deadline_until_terminal_handoff() {
    let probe = State::new(Vec::new());
    let registered = Registered::new(Arc::clone(&probe));
    let id: Arc<str> = Arc::from("pressure-rejection");
    let validation = worker(
        &registered.runtime,
        &registered.endpoint,
        echo_request(),
        None,
        registered.begin(Arc::clone(&id)),
        &NEVER_DEADLINES,
    );
    let WebSocketValidationOutcome::Prepared(mut prepared) = run(&registered.runtime, validation)
    else {
        panic!("query validation did not prepare")
    };
    let waker = Waker::from(Arc::new(NoopWake));
    let mut context = Context::from_waker(&waker);
    assert!(prepared.poll_setup_expired(&mut context).is_pending());

    let mut rejected = prepared
        .reject(GraphQlResponse::request_error(
            GraphQlError::new("Operation admission timed out").code("REQUEST_TIMEOUT"),
        ))
        .unwrap();
    let WebSocketTerminalMessage::Error(response) = rejected.take_message().unwrap() else {
        panic!("pending rejection did not select an error terminal")
    };
    assert_eq!(
        response
            .errors
            .first()
            .and_then(|error| error.extensions.get("code"))
            .and_then(chigiri_runtime::Value::as_str),
        Some("REQUEST_TIMEOUT")
    );
    let replacement = registered.connection.begin(id).unwrap();
    assert_ne!(rejected.generation(), replacement.generation());
    drop((rejected, replacement));
    registered.drain();
}

#[test]
fn validation_cancel_releases_id_before_its_bounded_worker_drain() {
    let probe = State::new(Vec::new());
    let registered = Registered::new(Arc::clone(&probe));
    let id: Arc<str> = Arc::from("cancel-validation");
    let mut validation = worker(
        &registered.runtime,
        &registered.endpoint,
        echo_request(),
        None,
        registered.begin(Arc::clone(&id)),
        &NEVER_DEADLINES,
    );
    validation.cancel();
    let replacement = registered.connection.begin(id).unwrap();
    assert!(matches!(
        run(&registered.runtime, validation),
        WebSocketValidationOutcome::Terminal(_)
    ));
    drop(replacement);
    registered.drain();
}

#[test]
fn one_shot_cancel_releases_id_before_its_bounded_worker_drain() {
    let probe = State::new(Vec::new());
    let registered = Registered::new(Arc::clone(&probe));
    let id: Arc<str> = Arc::from("cancel-one-shot");
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
    let mut outbox = SubscriptionOutbox::new(
        &registered.endpoint.subscriptions.settings,
        registered
            .endpoint
            .subscriptions
            .resources
            .delivery()
            .byte_owner(),
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::WebSocket,
        Some(Arc::clone(&id)),
    )
    .unwrap();
    let mut active = {
        let _entered = registered.runtime.enter();
        prepared.activate_one_shot(outbox.reserve_before_poll(0).unwrap())
    }
    .unwrap_or_else(|failure| panic!("query activation failed: {:?}", failure.error()));
    active.cancel();
    let replacement = registered.connection.begin(id).unwrap();
    assert!(matches!(
        run(&registered.runtime, active),
        WebSocketOneShotOutcome::Terminal(_)
    ));
    drop((replacement, outbox));
    registered.drain();
}

#[test]
fn subscription_setup_cancel_releases_id_before_its_bounded_worker_drain() {
    let probe = State::modes(
        Vec::new(),
        SetupMode::Pending,
        EventMode::Object,
        FactoryMode::Ready,
        FactoryMode::Ready,
        HookMode::Ready,
    );
    let registered = Registered::new(Arc::clone(&probe));
    let id: Arc<str> = Arc::from("cancel-setup");
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
    let setup = {
        let _entered = registered.runtime.enter();
        prepared.activate_subscription()
    }
    .unwrap_or_else(|failure| panic!("subscription activation failed: {:?}", failure.error()));
    let termination =
        setup.cancel(chigiri_runtime::__private::SubscriptionTermination::ClientCancelled);
    let replacement = registered.connection.begin(id).unwrap();
    let mut terminal = run(&registered.runtime, termination).unwrap();
    assert!(!terminal.permits_prior_event_drain());
    assert_eq!(
        run(
            &registered.runtime,
            poll_fn(|context| terminal.poll_cleanup(context))
        ),
        BridgeCleanupOutcome::Completed
    );
    drop(terminal);
    drop(replacement);
    registered.drain();
}

#[test]
fn joined_one_shot_context_panic_fails_without_repolling_a_completed_worker() {
    let probe = State::modes(
        Vec::new(),
        SetupMode::Ready,
        EventMode::Object,
        FactoryMode::Panic,
        FactoryMode::Ready,
        HookMode::Ready,
    );
    let registered = Registered::new(Arc::clone(&probe));
    let validation = worker(
        &registered.runtime,
        &registered.endpoint,
        echo_request(),
        None,
        registered.begin(Arc::from("panic-query")),
        &NEVER_DEADLINES,
    );
    let WebSocketValidationOutcome::Prepared(prepared) = run(&registered.runtime, validation)
    else {
        panic!("query validation did not prepare")
    };
    let mut outbox = SubscriptionOutbox::new(
        &registered.endpoint.subscriptions.settings,
        registered
            .endpoint
            .subscriptions
            .resources
            .delivery()
            .byte_owner(),
        SubscriptionOverflow::Backpressure,
        SubscriptionTransport::WebSocket,
        Some(Arc::from("panic-query")),
    )
    .unwrap();
    let active = {
        let _entered = registered.runtime.enter();
        prepared.activate_one_shot(outbox.reserve_before_poll(0).unwrap())
    }
    .unwrap_or_else(|failure| panic!("query active admission failed: {:?}", failure.error()));

    assert!(matches!(
        run(&registered.runtime, active),
        WebSocketOneShotOutcome::Terminal(_)
    ));
    assert_eq!(count(&probe.counts.factory_calls), 1);
    drop(outbox);
    registered.drain();
}

#[test]
fn joined_subscription_event_panic_fails_terminal_without_missing_worker_panic() {
    let probe = State::modes(
        vec![SourceStep::Event(3)],
        SetupMode::Ready,
        EventMode::Object,
        FactoryMode::Ready,
        FactoryMode::Panic,
        HookMode::Ready,
    );
    let registered = Registered::new(Arc::clone(&probe));
    let validation = worker(
        &registered.runtime,
        &registered.endpoint,
        harness::request(),
        None,
        registered.begin(Arc::from("panic-event")),
        &NEVER_DEADLINES,
    );
    let WebSocketValidationOutcome::Prepared(prepared) = run(&registered.runtime, validation)
    else {
        panic!("subscription validation did not prepare")
    };
    let WebSocketSubscriptionSetupOutcome::Ready {
        mut subscription,
        overflow,
    } = run(
        &registered.runtime,
        {
            let _entered = registered.runtime.enter();
            prepared.activate_subscription()
        }
        .unwrap_or_else(|failure| {
            panic!(
                "subscription active admission failed: {:?}",
                failure.error()
            )
        }),
    )
    else {
        panic!("subscription source did not become ready")
    };
    let mut outbox = SubscriptionOutbox::new(
        &registered.endpoint.subscriptions.settings,
        registered
            .endpoint
            .subscriptions
            .resources
            .delivery()
            .byte_owner(),
        overflow,
        SubscriptionTransport::WebSocket,
        Some(Arc::from("panic-event")),
    )
    .unwrap();
    assert!(
        subscription
            .request_event(outbox.reserve_before_poll(0).unwrap())
            .is_ok()
    );
    let mut terminal = run(
        &registered.runtime,
        poll_fn(|context| subscription.poll_event(context, &mut outbox)),
    )
    .unwrap()
    .into_terminal()
    .unwrap_or_else(|_| panic!("event panic did not select a terminal"));
    assert!(terminal.permits_prior_event_drain());
    assert_eq!(
        run(
            &registered.runtime,
            poll_fn(|context| terminal.poll_cleanup(context))
        ),
        BridgeCleanupOutcome::Completed
    );
    drop(terminal);
    drop(outbox);
    registered.drain();
}
