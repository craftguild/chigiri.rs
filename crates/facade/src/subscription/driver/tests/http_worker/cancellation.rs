//! Normal cancellation drives the same concrete task and local cleanup owner.

use std::future::poll_fn;
use std::sync::Arc;

use chigiri_http::ResponseCompletionOutcome;
use chigiri_runtime::LifecycleResult;

use super::super::state::{SourceStep, State, count};
use super::support::{VALID, drain, endpoint, pending, profiles, run, runtime};
use crate::transport::streaming::{HttpSubscriptionSignal, PreparedHttpSubscription};

fn check(demand: bool) {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new(vec![SourceStep::Pending]);
        let (endpoint, signals) = endpoint(Arc::clone(&probe));
        let bytes = endpoint.subscriptions.resources.delivery().byte_owner();
        let guard = endpoint
            .subscriptions
            .resources
            .register_worker_runtime()
            .unwrap();
        let request = run(
            &runtime,
            pending(Arc::clone(&endpoint), profile, VALID, false),
        );
        let PreparedHttpSubscription::Ready(ready) = run(&runtime, request.prepare()) else {
            panic!("source must commit before cancellation");
        };
        let (initial, mut outbox, mut events) = ready.into_parts();
        if demand {
            events
                .request_next(outbox.reserve_before_poll(0).unwrap())
                .unwrap_or_else(|_| panic!("one demand"));
            run(&runtime, async {
                while count(&probe.counts.source_polls) == 0 {
                    tokio::task::yield_now().await;
                }
            });
        }
        let before = bytes.used();
        events.cancel();
        events.cancel();
        assert_eq!(bytes.used(), before);
        assert!(matches!(
            run(&runtime, poll_fn(|context| events.poll_worker(context))),
            HttpSubscriptionSignal::Cancelled
        ));
        assert_eq!(count(&probe.counts.source_drops), 1);
        assert_eq!(count(&probe.counts.source_polls), usize::from(demand));
        assert_eq!(count(&probe.counts.completion_calls), 1);
        assert_eq!(
            *probe.completion_outcomes.lock().unwrap(),
            [ResponseCompletionOutcome::Encoded]
        );
        assert_eq!(signals.completed(), [(LifecycleResult::Succeeded, None)]);
        // The initial wire owner remains independently charged after source join.
        assert!(bytes.used() > 0);
        drop(initial);
        assert!(
            endpoint
                .subscriptions
                .resources
                .begin_http_subscription()
                .unwrap()
                .activate(crate::subscription::bridge_resources::BridgeOperationKind::Subscription)
                .is_ok()
        );
        let late = outbox.reserve_before_poll(0).unwrap();
        let held = bytes.used();
        let late = events
            .request_next(late)
            .expect_err("cancelled demand rejected with its token");
        assert_eq!(bytes.used(), held);
        drop(late);
        drop((outbox, events));
        drain(&runtime, guard);
        assert_eq!(bytes.used(), 0);
        assert_eq!(signals.completed().len(), 1);
    }
}

#[test]
fn idle_source_cancellation_joins_locally_without_prefetch_or_callback_retry() {
    check(false);
}

#[test]
fn demanded_pending_source_is_woken_and_dropped_inside_its_concrete_task() {
    check(true);
}
