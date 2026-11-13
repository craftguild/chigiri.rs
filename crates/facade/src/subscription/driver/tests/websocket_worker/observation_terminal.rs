//! Typed decisions freeze the common outbox before aborting the source task.

use std::future::poll_fn;
use std::sync::Arc;

use chigiri_http::SubscriptionTransport;
use chigiri_runtime::__private::SubscriptionTermination;

use super::super::harness;
use super::super::observation::{Probe, install};
use super::super::state::{SourceStep, State};
use super::support::{NEVER_DEADLINES, Registered, run, worker};
use crate::subscription::outbox::{SubscriptionOutbox, SubscriptionOutboxSignal};
use crate::subscription::settings::ValidatedSubscriptionSettings;
use crate::subscription::worker::websocket::{
    ActiveWebSocketSubscription, WebSocketSubscriptionSetupOutcome, WebSocketValidationOutcome,
};

fn active(
    probe: Arc<State>,
) -> (
    Registered,
    ActiveWebSocketSubscription,
    SubscriptionOutbox,
    Arc<Probe>,
) {
    let host = install(&probe);
    let registered = Registered::new(probe);
    let id: Arc<str> = Arc::from("observed-terminal");
    let validation = worker(
        &registered.runtime,
        &registered.endpoint,
        harness::request(),
        None,
        registered.begin(id.clone()),
        &NEVER_DEADLINES,
    );
    let WebSocketValidationOutcome::Prepared(prepared) = run(&registered.runtime, validation)
    else {
        panic!("validation must succeed")
    };
    let setup = {
        let _entered = registered.runtime.enter();
        prepared
            .activate_subscription()
            .unwrap_or_else(|_| panic!("activation must succeed"))
    };
    let WebSocketSubscriptionSetupOutcome::Ready {
        subscription,
        overflow,
    } = run(&registered.runtime, setup)
    else {
        panic!("setup must succeed")
    };
    let mut limits = *registered.endpoint.subscriptions.settings.limits();
    limits.max_queued_events = 2;
    let settings = ValidatedSubscriptionSettings::new(
        limits,
        *registered.endpoint.subscriptions.settings.protocol(),
    );
    let outbox = SubscriptionOutbox::new(
        &settings,
        registered
            .endpoint
            .subscriptions
            .resources
            .delivery()
            .byte_owner(),
        overflow,
        SubscriptionTransport::WebSocket,
        Some(id),
    )
    .unwrap();
    (registered, *subscription, outbox, host)
}

#[test]
fn websocket_observed_source_terminal_drains_exact_evictions_then_freezes_before_source_drop() {
    for failed in [false, true] {
        let mut source: Vec<_> = (1..=5).map(SourceStep::Event).collect();
        if failed {
            source.push(SourceStep::Error);
        }
        let (registered, mut subscription, mut outbox, host) = active(State::new(source));
        let mut terminal = loop {
            let reservation = outbox.reserve_before_poll(0).unwrap();
            assert!(subscription.request_event(reservation).is_ok());
            let result = run(
                &registered.runtime,
                poll_fn(|context| subscription.poll_event(context, &mut outbox)),
            )
            .unwrap();
            match result.into_event() {
                Ok((reservation, response)) => outbox
                    .encode_and_enqueue(reservation, &response, 0)
                    .unwrap(),
                Err(result) => {
                    break result
                        .into_terminal()
                        .unwrap_or_else(|_| panic!("expected terminal"));
                }
            }
        };
        let decision = if failed {
            SubscriptionTermination::ExecutionFailed
        } else {
            SubscriptionTermination::SourceComplete
        };
        assert_eq!(*host.terminals.lock().unwrap(), [decision]);
        assert_eq!(*host.totals.lock().unwrap(), [3]);
        assert_eq!(*host.drops.lock().unwrap(), [1, 2, 3]);
        assert_eq!(*host.source_drops_at_finish.lock().unwrap(), [0]);
        assert_eq!(
            outbox.reserve_before_poll(0).err(),
            Some(SubscriptionOutboxSignal::Cancelled)
        );
        run(
            &registered.runtime,
            poll_fn(|context| terminal.poll_cleanup(context)),
        );
        drop((subscription, terminal, outbox));
        registered.drain();
        assert_eq!(*host.terminals.lock().unwrap(), [decision]);
        assert_eq!(*host.totals.lock().unwrap(), [3]);
    }
}

#[test]
fn websocket_explicit_cancellation_classes_survive_late_drop_and_cleanup() {
    for decision in [
        SubscriptionTermination::ClientCancelled,
        SubscriptionTermination::TransportClosed,
        SubscriptionTermination::ServerShutdown,
    ] {
        let (registered, subscription, mut outbox, host) =
            active(State::new(vec![SourceStep::Pending]));
        let mut terminal = run(
            &registered.runtime,
            subscription.cancel(decision, Some(&mut outbox)),
        )
        .unwrap();
        assert_eq!(*host.terminals.lock().unwrap(), [decision]);
        assert_eq!(*host.totals.lock().unwrap(), [0]);
        assert_eq!(*host.source_drops_at_finish.lock().unwrap(), [0]);
        run(
            &registered.runtime,
            poll_fn(|context| terminal.poll_cleanup(context)),
        );
        drop((terminal, outbox));
        registered.drain();
        assert_eq!(*host.terminals.lock().unwrap(), [decision]);
    }
}

#[test]
fn websocket_typed_delivery_and_invariant_failures_are_not_inferred_from_wire_errors() {
    for (signal, decision) in [
        (
            SubscriptionOutboxSignal::OversizedEvent,
            SubscriptionTermination::DeliveryFailed,
        ),
        (
            SubscriptionOutboxSignal::Invariant,
            SubscriptionTermination::Internal,
        ),
    ] {
        let (registered, mut subscription, mut outbox, host) =
            active(State::new(vec![SourceStep::Pending]));
        let mut terminal = subscription
            .delivery_failure(signal, Some(&mut outbox))
            .unwrap();
        assert_eq!(*host.terminals.lock().unwrap(), [decision]);
        assert_eq!(*host.source_drops_at_finish.lock().unwrap(), [0]);
        run(
            &registered.runtime,
            poll_fn(|context| terminal.poll_cleanup(context)),
        );
        drop((subscription, terminal, outbox));
        registered.drain();
        assert_eq!(*host.terminals.lock().unwrap(), [decision]);
        assert_eq!(*host.totals.lock().unwrap(), [0]);
    }
}

#[test]
fn websocket_logical_terminal_precedes_the_real_blocked_source_destructor() {
    let probe = State::new(Vec::new());
    let gate = probe.block_source_drop();
    let (registered, mut subscription, mut outbox, host) = active(probe.clone());
    let reservation = outbox.reserve_before_poll(0).unwrap();
    assert!(subscription.request_event(reservation).is_ok());
    let result = run(
        &registered.runtime,
        poll_fn(|context| subscription.poll_event(context, &mut outbox)),
    )
    .unwrap();
    let mut terminal = result
        .into_terminal()
        .unwrap_or_else(|_| panic!("source must complete"));
    gate.wait_entered();
    assert_eq!(
        *host.terminals.lock().unwrap(),
        [SubscriptionTermination::SourceComplete]
    );
    assert_eq!(*host.source_drops_at_finish.lock().unwrap(), [0]);
    let _entered = registered.runtime.enter();
    assert!(
        terminal
            .poll_cleanup(&mut std::task::Context::from_waker(std::task::Waker::noop()))
            .is_pending()
    );
    gate.release();
    run(
        &registered.runtime,
        poll_fn(|context| terminal.poll_cleanup(context)),
    );
    drop((subscription, terminal, outbox, _entered));
    registered.drain();
    assert_eq!(
        *host.terminals.lock().unwrap(),
        [SubscriptionTermination::SourceComplete]
    );
}
