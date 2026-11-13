//! Actual committed wire ownership shares, but never duplicates, active quota.

use std::future::poll_fn;
use std::sync::Arc;

use chigiri_http::{
    BoxFuture, HookError, ResponseHeaders, ResponseHook, ResponseHookContext, SubscriptionTransport,
};
use chigiri_runtime::LifecycleResult;

use super::super::state::{AppContext, HookState, SourceStep, State, count};
use super::support::{Signals, VALID, drain, endpoint, pending, profiles, run, runtime};
use crate::transport::EndpointState;
use crate::transport::streaming::{
    HttpSubscriptionEvents, HttpSubscriptionSignal, InitialCommitTestFault,
    PreparedHttpSubscription, ReadyHttpSubscription,
};

struct RepeatedHook(Arc<State>);

impl ResponseHook<HookState> for RepeatedHook {
    fn call<'a>(
        &'a self,
        _: ResponseHookContext<'a, HookState>,
    ) -> BoxFuture<'a, Result<ResponseHeaders, HookError>> {
        let call = self
            .0
            .counts
            .hook_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        assert_eq!(count(&self.0.counts.setup_context_drops), call);
        Box::pin(async {
            let mut headers = ResponseHeaders::new();
            headers.append("x-subscription", "ready").unwrap();
            Ok(headers)
        })
    }
}

fn reusable_endpoint(probe: Arc<State>) -> (Arc<EndpointState<AppContext>>, Arc<Signals>) {
    let (mut endpoint, signals) = endpoint(Arc::clone(&probe));
    let endpoint_state = Arc::get_mut(&mut endpoint).unwrap();
    let hook = crate::transport::erase_hook(RepeatedHook(probe));
    endpoint_state.response_hook = Some(Arc::clone(&hook));
    Arc::get_mut(&mut endpoint_state.subscription_execution)
        .unwrap()
        .response_hook = Some(hook);
    (endpoint, signals)
}

fn ready(
    runtime: &tokio::runtime::Runtime,
    endpoint: &Arc<EndpointState<AppContext>>,
    profile: SubscriptionTransport,
) -> ReadyHttpSubscription {
    let request = run(
        runtime,
        pending(Arc::clone(endpoint), profile, VALID, false),
    );
    match run(runtime, request.prepare()) {
        PreparedHttpSubscription::Ready(ready) => ready,
        PreparedHttpSubscription::Response(response) => panic!(
            "one admitted source, got {response:?}: {}",
            std::str::from_utf8(response.as_ref()).unwrap()
        ),
        PreparedHttpSubscription::Failed(error) => panic!("one admitted source, closed {error:?}"),
    }
}

fn reject(
    runtime: &tokio::runtime::Runtime,
    endpoint: &Arc<EndpointState<AppContext>>,
    profile: SubscriptionTransport,
) {
    let request = run(
        runtime,
        pending(Arc::clone(endpoint), profile, VALID, false),
    );
    let PreparedHttpSubscription::Response(response) = run(runtime, request.prepare()) else {
        panic!("one-slot capacity must reject the replacement");
    };
    assert_eq!(response.status(), 503);
    assert!(
        std::str::from_utf8(response.as_ref())
            .unwrap()
            .contains("SUBSCRIPTION_LIMIT_EXCEEDED")
    );
    assert!(
        !response
            .headers()
            .iter()
            .any(|header| header.name == "retry-after")
    );
}

fn cancel(runtime: &tokio::runtime::Runtime, events: &mut HttpSubscriptionEvents) {
    events.cancel();
    assert!(matches!(
        run(runtime, poll_fn(|context| events.poll_worker(context))),
        HttpSubscriptionSignal::Cancelled
    ));
}

#[test]
fn joined_source_does_not_release_capacity_before_the_last_initial_reference() {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new(vec![SourceStep::Pending]);
        let (endpoint, signals) = reusable_endpoint(Arc::clone(&probe));
        let guard = endpoint
            .subscriptions
            .resources
            .register_worker_runtime()
            .unwrap();
        let bytes = endpoint.subscriptions.resources.delivery().byte_owner();
        let (initial, outbox, mut events) = ready(&runtime, &endpoint, profile).into_parts();
        let initial = Arc::new(initial);
        let second = Arc::clone(&initial);
        let third = Arc::clone(&initial);
        cancel(&runtime, &mut events);
        assert_eq!(count(&probe.counts.source_drops), 1);
        let wire_bytes = bytes.used();
        assert!(wire_bytes > 0);
        drop(initial);
        reject(&runtime, &endpoint, profile);
        assert_eq!(bytes.used(), wire_bytes);
        drop(second);
        reject(&runtime, &endpoint, profile);
        assert_eq!(count(&probe.counts.setup_calls), 1);
        assert_eq!(count(&probe.counts.completion_calls), 1);
        assert_eq!(bytes.used(), wire_bytes);
        drop(third);
        assert_eq!(bytes.used(), 0);
        assert_eq!(
            signals.completed(),
            [
                (LifecycleResult::Succeeded, None),
                (
                    LifecycleResult::Rejected,
                    Some("SUBSCRIPTION_LIMIT_EXCEEDED")
                ),
                (
                    LifecycleResult::Rejected,
                    Some("SUBSCRIPTION_LIMIT_EXCEEDED")
                ),
            ]
        );
        let replacement = ready(&runtime, &endpoint, profile);
        assert_eq!(count(&probe.counts.setup_calls), 2);
        assert_eq!(count(&probe.counts.source_polls), 0);
        drop((replacement, outbox, events));
        drain(&runtime, guard);
        assert_eq!(count(&probe.counts.source_drops), 2);
        assert_eq!(bytes.used(), 0);
    }
}

#[test]
fn initial_reference_first_still_retains_capacity_until_concrete_source_join() {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new(vec![SourceStep::Pending]);
        let (endpoint, _) = reusable_endpoint(Arc::clone(&probe));
        let guard = endpoint
            .subscriptions
            .resources
            .register_worker_runtime()
            .unwrap();
        let bytes = endpoint.subscriptions.resources.delivery().byte_owner();
        let (initial, outbox, mut events) = ready(&runtime, &endpoint, profile).into_parts();
        drop(initial);
        assert_eq!(bytes.used(), 0);
        reject(&runtime, &endpoint, profile);
        assert_eq!(count(&probe.counts.source_drops), 0);
        assert_eq!(count(&probe.counts.setup_calls), 1);
        cancel(&runtime, &mut events);
        assert_eq!(count(&probe.counts.source_drops), 1);
        let replacement = ready(&runtime, &endpoint, profile);
        drop((replacement, outbox, events));
        drain(&runtime, guard);
        assert_eq!(bytes.used(), 0);
    }
}

#[test]
fn a_retained_initial_wire_owner_does_not_consume_another_endpoints_slot() {
    let runtime = runtime();
    let profile = SubscriptionTransport::ServerSentEvents;
    let probe_a = State::new(Vec::new());
    let probe_b = State::new(Vec::new());
    let (endpoint_a, _) = endpoint(Arc::clone(&probe_a));
    let (endpoint_b, _) = endpoint(Arc::clone(&probe_b));
    let guard_a = endpoint_a
        .subscriptions
        .resources
        .register_worker_runtime()
        .unwrap();
    let guard_b = endpoint_b
        .subscriptions
        .resources
        .register_worker_runtime()
        .unwrap();
    let (initial, outbox, mut events) = ready(&runtime, &endpoint_a, profile).into_parts();
    cancel(&runtime, &mut events);
    reject(&runtime, &endpoint_a, profile);
    let separate = ready(&runtime, &endpoint_b, profile);
    assert_eq!(count(&probe_b.counts.setup_calls), 1);
    drop((initial, outbox, events, separate));
    drain(&runtime, guard_a);
    drain(&runtime, guard_b);
    assert_eq!(
        endpoint_a
            .subscriptions
            .resources
            .delivery()
            .byte_owner()
            .used(),
        0
    );
    assert_eq!(
        endpoint_b
            .subscriptions
            .resources
            .delivery()
            .byte_owner()
            .used(),
        0
    );
}

#[test]
fn failed_commit_does_not_leave_a_physical_lease_in_finite_or_closed_output() {
    let runtime = runtime();
    for profile in profiles() {
        for fault in [
            InitialCommitTestFault::Commit,
            InitialCommitTestFault::ShrinkOnce,
            InitialCommitTestFault::ShrinkAlways(Arc::default()),
            InitialCommitTestFault::MissingBuffer,
            InitialCommitTestFault::CorruptedBytes,
        ] {
            let probe = State::new(Vec::new());
            let (endpoint, _) = endpoint(Arc::clone(&probe));
            let guard = endpoint
                .subscriptions
                .resources
                .register_worker_runtime()
                .unwrap();
            let request = run(
                &runtime,
                pending(Arc::clone(&endpoint), profile, VALID, false),
            );
            let prepared = run(&runtime, request.with_commit_fault(fault).prepare());
            match &prepared {
                PreparedHttpSubscription::Response(response) => assert_eq!(response.status(), 500),
                PreparedHttpSubscription::Failed(_) => {}
                PreparedHttpSubscription::Ready(_) => {
                    panic!("failed commit cannot publish a source")
                }
            }
            drain(&runtime, guard);
            assert_eq!(count(&probe.counts.source_drops), 1);
            assert_eq!(count(&probe.counts.source_polls), 0);
            let replacement = endpoint
                .subscriptions
                .resources
                .begin_http_subscription()
                .unwrap()
                .activate(crate::subscription::bridge_resources::BridgeOperationKind::Subscription)
                .expect("failed publication retains no writer lease after source join");
            drop((replacement, prepared));
            assert_eq!(
                endpoint
                    .subscriptions
                    .resources
                    .delivery()
                    .byte_owner()
                    .used(),
                0
            );
        }
    }
}
