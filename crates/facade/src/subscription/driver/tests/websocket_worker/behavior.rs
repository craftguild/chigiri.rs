use std::future::poll_fn;
use std::sync::Arc;
use std::time::Duration;

use chigiri_http::{RequestPhase, SubscriptionTransport, WebSocketOneShot};
use chigiri_runtime::__private::WebSocketOperationKind;
use chigiri_runtime::{SubscriptionOverflow, Value};

use super::super::harness;
use super::super::state::{SourceStep, State, count};
use super::support::{NEVER_DEADLINES, Registered, TokioDeadlines, echo_request, run, worker};
use crate::subscription::outbox::SubscriptionOutbox;
use crate::subscription::worker::websocket::{
    WebSocketOneShotOutcome, WebSocketSubscriptionSetupOutcome, WebSocketTerminalMessage,
    WebSocketValidationOutcome,
};

#[test]
fn joined_query_uses_fresh_websocket_context_and_bounded_outer_reservation() {
    let probe = State::new(Vec::new());
    let registered = Registered::new(Arc::clone(&probe));
    let id: Arc<str> = Arc::from("query-id");
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
        panic!("query validation did not join with a prepared operation")
    };
    assert_eq!(prepared.kind(), WebSocketOperationKind::Query);
    assert_eq!(count(&probe.counts.scalar_calls), 1);
    assert_eq!(count(&probe.counts.factory_calls), 0);

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
    let reservation = outbox.reserve_before_poll(0).unwrap();
    let active = {
        let _entered = registered.runtime.enter();
        prepared.activate_one_shot(reservation)
    }
    .unwrap_or_else(|failure| panic!("query active admission failed: {:?}", failure.error()));
    let WebSocketOneShotOutcome::Ready(mut joined) = run(&registered.runtime, active) else {
        panic!("query execution did not join with one response")
    };
    assert!(joined.terminal().unwrap().permits_prior_event_drain());
    let (reservation, response) = joined.take_event().unwrap();
    let Value::Object(data) = response.data.as_ref().unwrap() else {
        panic!("query response was not an object")
    };
    assert_eq!(data.get("echo"), Some(&Value::from("worker-value")));
    outbox
        .encode_and_enqueue(reservation, &response, 0)
        .unwrap();
    let (frame, mut progress) = outbox.begin_send(0).unwrap().unwrap();
    assert!(
        std::str::from_utf8(frame.as_bytes())
            .unwrap()
            .contains("worker-value")
    );
    assert!(progress.accept(frame.len(), 0).unwrap());
    drop(frame);
    outbox.finish_send(progress, 0).unwrap();

    let mut terminal = joined.take_terminal().unwrap();
    let shared_physical = terminal.physical_lease().unwrap();
    let physical = terminal.take_physical().unwrap();
    assert_eq!(physical.generation(), terminal.generation());
    assert_eq!(shared_physical.generation(), terminal.generation());
    assert!(matches!(
        terminal.take_message(),
        Some(WebSocketTerminalMessage::Complete)
    ));
    assert!(!terminal.permits_prior_event_drain());
    let replacement = registered.connection.begin(Arc::clone(&id)).unwrap();
    assert_ne!(replacement.generation(), terminal.generation());
    drop((replacement, terminal, physical, shared_physical, outbox));
    assert_eq!(count(&probe.counts.factory_calls), 1);
    assert_eq!(count(&probe.counts.hook_calls), 0);
    assert_eq!(count(&probe.counts.completion_calls), 0);
    assert_eq!(count(&probe.counts.completion_drops), 1);
    assert_eq!(
        *probe.phases.lock().unwrap(),
        [RequestPhase::WebSocketOperation {
            operation: WebSocketOneShot::Query,
        }]
    );
    registered.drain();
}

#[test]
fn subscription_setup_and_events_use_fresh_contexts_without_http_callbacks() {
    let probe = State::new(vec![SourceStep::Event(7)]);
    let registered = Registered::new(Arc::clone(&probe));
    let id: Arc<str> = Arc::from("subscription-id");
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
    assert_eq!(prepared.kind(), WebSocketOperationKind::Subscription);
    assert_eq!(count(&probe.counts.factory_calls), 0);

    let setup = {
        let _entered = registered.runtime.enter();
        prepared.activate_subscription()
    }
    .unwrap_or_else(|failure| {
        panic!(
            "subscription active admission failed: {:?}",
            failure.error()
        )
    });
    let WebSocketSubscriptionSetupOutcome::Ready {
        mut subscription,
        overflow,
    } = run(&registered.runtime, setup)
    else {
        panic!("subscription source did not become ready")
    };
    assert!(subscription.emission_lease().unwrap().can_emit());
    let physical = subscription.physical_lease().unwrap();
    assert_eq!(overflow, SubscriptionOverflow::DropOldest);
    assert_eq!(count(&probe.counts.source_polls), 0);
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
        Some(Arc::clone(&id)),
    )
    .unwrap();

    assert!(
        subscription
            .request_event(outbox.reserve_before_poll(0).unwrap())
            .is_ok()
    );
    let event = run(
        &registered.runtime,
        poll_fn(|context| subscription.poll_event(context, &mut outbox)),
    )
    .unwrap();
    let (reservation, response) = event
        .into_event()
        .unwrap_or_else(|_| panic!("first demanded result was not an event"));
    outbox
        .encode_and_enqueue(reservation, &response, 0)
        .unwrap();
    let (frame, mut progress) = outbox.begin_send(0).unwrap().unwrap();
    assert!(
        std::str::from_utf8(frame.as_bytes())
            .unwrap()
            .contains("\"value\":7")
    );
    assert!(progress.accept(frame.len(), 0).unwrap());
    drop(frame);
    outbox.finish_send(progress, 0).unwrap();

    assert!(
        subscription
            .request_event(outbox.reserve_before_poll(1).unwrap())
            .is_ok()
    );
    let terminal_event = run(
        &registered.runtime,
        poll_fn(|context| subscription.poll_event(context, &mut outbox)),
    )
    .unwrap();
    let mut terminal = terminal_event
        .into_terminal()
        .unwrap_or_else(|_| panic!("natural source end did not return completion"));
    assert!(terminal.permits_prior_event_drain());
    let shared_terminal_physical = terminal.physical_lease().unwrap();
    let terminal_physical = terminal.take_physical().unwrap();
    assert_eq!(terminal_physical.generation(), terminal.generation());
    assert_eq!(physical.generation(), terminal.generation());
    assert!(matches!(
        terminal.take_message(),
        Some(WebSocketTerminalMessage::Complete)
    ));
    assert!(!terminal.permits_prior_event_drain());
    let replacement = registered.connection.begin(id).unwrap();
    assert_ne!(replacement.generation(), terminal.generation());
    drop((
        terminal,
        terminal_physical,
        shared_terminal_physical,
        physical,
        replacement,
        outbox,
    ));

    assert_eq!(count(&probe.counts.setup_calls), 1);
    assert_eq!(count(&probe.counts.source_polls), 2);
    assert_eq!(count(&probe.counts.source_drops), 1);
    assert_eq!(count(&probe.counts.factory_calls), 2);
    assert_eq!(count(&probe.counts.hook_calls), 0);
    assert_eq!(count(&probe.counts.completion_calls), 0);
    assert_eq!(count(&probe.counts.completion_drops), 2);
    registered.drain();
}

#[test]
fn original_setup_deadline_remains_external_to_blocked_scalar_coercion() {
    let probe = State::new(Vec::new());
    let gate = probe.block_scalar();
    let registered = Registered::new(Arc::clone(&probe));
    let deadlines = Box::leak(Box::new(TokioDeadlines::default()));
    let validation = worker(
        &registered.runtime,
        &registered.endpoint,
        echo_request(),
        None,
        registered.begin(Arc::from("blocked-scalar")),
        deadlines,
    );
    let release = Arc::clone(&gate);
    let releaser = std::thread::spawn(move || {
        assert!(release.wait_entered(Duration::from_secs(2)));
        std::thread::sleep(Duration::from_millis(50));
        release.release();
    });

    assert!(matches!(
        run(&registered.runtime, validation),
        WebSocketValidationOutcome::Terminal(_)
    ));
    releaser.join().unwrap();
    assert_eq!(
        deadlines
            .starts()
            .into_iter()
            .filter(|duration| *duration == super::super::deadlines::SETUP_TIMEOUT)
            .count(),
        1
    );
    assert_eq!(count(&probe.counts.scalar_calls), 1);
    assert_eq!(count(&probe.counts.factory_calls), 0);
    registered.drain();
}

#[test]
fn demanded_pending_source_cancellation_returns_its_reservation_after_join() {
    let probe = State::new(vec![SourceStep::Pending]);
    let registered = Registered::new(Arc::clone(&probe));
    let validation = worker(
        &registered.runtime,
        &registered.endpoint,
        harness::request(),
        None,
        registered.begin(Arc::from("cancel-source")),
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
    let bytes = registered
        .endpoint
        .subscriptions
        .resources
        .delivery()
        .byte_owner();
    let mut outbox = SubscriptionOutbox::new(
        &registered.endpoint.subscriptions.settings,
        bytes.clone(),
        overflow,
        SubscriptionTransport::WebSocket,
        Some(Arc::from("cancel-source")),
    )
    .unwrap();
    assert!(
        subscription
            .request_event(outbox.reserve_before_poll(0).unwrap())
            .is_ok()
    );
    let wait_started = std::time::Instant::now();
    while count(&probe.counts.source_polls) == 0 {
        assert!(
            wait_started.elapsed() < Duration::from_secs(2),
            "source demand did not reach the concrete worker"
        );
        std::thread::yield_now();
    }
    let mut terminal = run(
        &registered.runtime,
        subscription.cancel(
            chigiri_runtime::__private::SubscriptionTermination::ClientCancelled,
            Some(&mut outbox),
        ),
    )
    .unwrap();
    assert!(!terminal.permits_prior_event_drain());
    assert_eq!(
        run(
            &registered.runtime,
            poll_fn(|context| terminal.poll_cleanup(context))
        ),
        crate::subscription::bridge_resources::BridgeCleanupOutcome::Completed
    );

    assert_eq!(count(&probe.counts.source_polls), 1);
    assert_eq!(count(&probe.counts.source_drops), 1);
    assert_eq!(bytes.used(), 0);
    drop(terminal);
    drop(outbox);
    registered.drain();
}
