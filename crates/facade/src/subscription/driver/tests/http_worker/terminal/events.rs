//! Idle and demanded event deadline behavior.

use std::future::poll_fn;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chigiri_http::SubscriptionTransport;

use super::application::{BlockingGate, SourceBehavior, install_source};
use super::support::{
    DeadlineFixture, VALID, drain, endpoint, pending_with_deadlines, run, runtime,
};
use crate::subscription::driver::tests::state::{State, count};
use crate::transport::streaming::{HttpSubscriptionSignal, PreparedHttpSubscription};

#[test]
fn idle_source_arms_no_event_timer_and_explicit_cancel_joins_promptly() {
    let fixture = DeadlineFixture::new();
    let runtime = runtime();
    let state = State::new(Vec::new());
    let (endpoint, _) = endpoint(Arc::clone(&state));
    let guard = endpoint
        .subscriptions
        .resources
        .register_worker_runtime()
        .unwrap();
    let pending = run(
        &runtime,
        pending_with_deadlines(
            Arc::clone(&endpoint),
            SubscriptionTransport::ServerSentEvents,
            VALID,
            fixture.deadlines,
        ),
    );
    let PreparedHttpSubscription::Ready(ready) = run(&runtime, pending.prepare()) else {
        panic!("idle source must prepare");
    };
    let (initial, outbox, mut events) = ready.into_parts();
    assert_eq!(fixture.deadlines.event.creations(), 0);
    assert_eq!(count(&state.counts.source_polls), 0);
    events.cancel();
    assert!(matches!(
        run(&runtime, poll_fn(|context| events.poll_worker(context))),
        HttpSubscriptionSignal::Cancelled
    ));
    assert_eq!(fixture.deadlines.event.creations(), 0);
    drop((initial, outbox, events));
    drain(&runtime, guard);
    assert_eq!(count(&state.counts.source_drops), 1);
}

#[test]
fn demanded_blocking_event_times_out_externally_and_retains_reservation_until_join() {
    let fixture = DeadlineFixture::new();
    let runtime = runtime();
    let state = State::new(Vec::new());
    let event_gate = BlockingGate::new();
    let (mut endpoint, signals) = endpoint(Arc::clone(&state));
    install_source(
        &mut endpoint,
        Arc::clone(&state),
        SourceBehavior::BlockingEvent(Arc::clone(&event_gate)),
    );
    signals.attach(&mut endpoint);
    let guard = endpoint
        .subscriptions
        .resources
        .register_worker_runtime()
        .unwrap();
    let pending = run(
        &runtime,
        pending_with_deadlines(
            Arc::clone(&endpoint),
            SubscriptionTransport::Multipart,
            VALID,
            fixture.deadlines,
        ),
    );
    let PreparedHttpSubscription::Ready(ready) = run(&runtime, pending.prepare()) else {
        panic!("blocking event source must prepare");
    };
    let (initial, mut outbox, mut events) = ready.into_parts();
    drop(initial);
    assert_eq!(fixture.deadlines.event.creations(), 0);
    let reservation = outbox.reserve_before_poll(0).unwrap();
    assert!(events.request_next(reservation).is_ok());
    event_gate.wait_entered();
    assert_eq!(fixture.deadlines.event.creations(), 1);
    fixture.deadlines.event.trigger();
    let bytes = endpoint.subscriptions.resources.delivery().byte_owner();
    let reserved = bytes.used();
    assert!(reserved > 0);
    let started = Instant::now();
    let signal = runtime.block_on(async {
        tokio::time::timeout(
            Duration::from_secs(7),
            poll_fn(|context| events.poll_worker(context)),
        )
        .await
        .expect("real five-second cleanup bound")
    });
    assert!(matches!(signal, HttpSubscriptionSignal::TimedOut));
    assert!(started.elapsed() >= Duration::from_millis(4_500));
    assert_eq!(bytes.used(), reserved);
    assert_eq!(count(&state.counts.event_drops), 0);
    event_gate.release();
    drop((outbox, events));
    drain(&runtime, guard);
    assert_eq!(bytes.used(), 0);
    assert_eq!(count(&state.counts.source_drops), 1);
}
