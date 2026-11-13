//! Multi-slot failure, fairness, shutdown, and physical-owner proofs.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use chigiri_http::SubscriptionTransport;

use super::super::super::state::{SourceStep, State, count};
use super::super::support::{self as worker, drain, run, runtime};
use super::aggregate_support::{AggregateSupervision, attach_aggregate, next_accepted};
use super::support::{
    active_endpoint, drive_until, drive_until_sources_pending, poll_body_once, text,
};
use crate::transport::streaming::PreparedHttpSubscription;
use crate::transport::streaming::supervisor::HttpStreamingError;

const PROFILES: [SubscriptionTransport; 2] = [
    SubscriptionTransport::ServerSentEvents,
    SubscriptionTransport::Multipart,
];

#[test]
fn aggregate_fatal_failure_reaches_both_committed_bodies_and_never_reopens() {
    let runtime = runtime();
    let probe = State::new(vec![SourceStep::Pending]);
    let (mut live, signals) = attach_aggregate(
        &runtime,
        Arc::clone(&probe),
        &PROFILES,
        Duration::from_secs(60),
        false,
    );
    let bytes = live
        .endpoint
        .subscriptions
        .resources
        .delivery()
        .byte_owner();
    let [body_a, body_b] = live.bodies.as_mut_slice() else {
        panic!("two aggregate bodies")
    };
    let held_a = next_accepted(&runtime, &mut live.driver, body_a).unwrap();
    let held_b = next_accepted(&runtime, &mut live.driver, body_b).unwrap();
    drive_until_sources_pending(&runtime, &mut live.driver, &probe, 2);

    live.driver.as_ref().get_ref().poison_registry_for_test();
    assert_eq!(
        run(&runtime, live.driver.as_mut()),
        Err(HttpStreamingError::Invariant)
    );
    assert_eq!(
        run(&runtime, live.driver.as_mut()),
        Err(HttpStreamingError::Invariant)
    );
    for body in &mut live.bodies {
        assert!(matches!(
            poll_body_once(&runtime, body),
            Poll::Ready(Some(Err(HttpStreamingError::Invariant)))
        ));
        assert!(matches!(poll_body_once(&runtime, body), Poll::Ready(None)));
    }
    assert_eq!(active_endpoint(&live.endpoint), 2);
    assert!(bytes.used() > 0);

    drop(live.bodies);
    assert_eq!(active_endpoint(&live.endpoint), 2);
    drain(&runtime, live.guard);
    wait_for_count(&runtime, &probe.counts.source_drops, 2);
    assert_eq!(signals.completed().len(), 2);
    assert_eq!(active_endpoint(&live.endpoint), 2);
    drop(held_a);
    wait_for_active(&runtime, &live.endpoint, 1);
    drop(held_b);
    wait_for_active(&runtime, &live.endpoint, 0);
    assert_eq!(active_endpoint(&live.endpoint), 0);

    let replacement_guard = live
        .endpoint
        .subscriptions
        .resources
        .register_worker_runtime()
        .unwrap();
    let pending = run(
        &runtime,
        worker::pending(
            Arc::clone(&live.endpoint),
            SubscriptionTransport::ServerSentEvents,
            worker::VALID,
            false,
        ),
    );
    let ready = match run(&runtime, pending.prepare()) {
        PreparedHttpSubscription::Ready(ready) => ready,
        PreparedHttpSubscription::Response(response) => {
            panic!("post-failure setup returned {response:?}")
        }
        PreparedHttpSubscription::Failed(error) => {
            panic!("post-failure setup failed: {error}")
        }
    };
    assert!(matches!(
        live.registration.attach(ready),
        Err(HttpStreamingError::Closed | HttpStreamingError::Invariant)
    ));
    drop(live.registration);
    drop(live.driver);
    drain(&runtime, replacement_guard);
    assert_eq!(count(&probe.counts.source_drops), 3);
    assert_eq!(signals.completed().len(), 3);
    assert_eq!(active_endpoint(&live.endpoint), 0);
    assert_eq!(bytes.used(), 0);
}

#[test]
fn both_slots_progress_and_last_registration_drop_drains_cleanly() {
    let runtime = runtime();
    let probe = State::new(vec![
        SourceStep::Event(1),
        SourceStep::Event(2),
        SourceStep::Pending,
    ]);
    let (mut live, signals) = attach_aggregate(
        &runtime,
        Arc::clone(&probe),
        &PROFILES,
        Duration::from_secs(60),
        false,
    );
    let bytes = live
        .endpoint
        .subscriptions
        .resources
        .delivery()
        .byte_owner();

    drive_until(&runtime, &mut live.driver, || {
        count(&probe.counts.nested_calls) == 4
    });
    let [body_a, body_b] = live.bodies.as_mut_slice() else {
        panic!("two aggregate bodies")
    };
    drop(next_accepted(&runtime, &mut live.driver, body_a).unwrap());
    drop(next_accepted(&runtime, &mut live.driver, body_b).unwrap());
    for body in [body_a, body_b] {
        let first = next_accepted(&runtime, &mut live.driver, body).unwrap();
        assert!(text(&first).contains("\"value\":1"));
        drop(first);
    }
    let [body_a, body_b] = live.bodies.as_mut_slice() else {
        unreachable!()
    };
    let held_a = next_accepted(&runtime, &mut live.driver, body_a).unwrap();
    let held_b = next_accepted(&runtime, &mut live.driver, body_b).unwrap();
    assert!(text(&held_a).contains("\"value\":2"));
    assert!(text(&held_b).contains("\"value\":2"));
    drive_until_sources_pending(&runtime, &mut live.driver, &probe, 2);

    let AggregateSupervision {
        endpoint,
        registration,
        mut driver,
        mut bodies,
        guard,
    } = live;
    let last_registration = registration.clone();
    drop(registration);
    {
        let _entered = runtime.enter();
        assert!(matches!(
            driver
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        ));
    }
    drop(last_registration);
    run(&runtime, driver.as_mut()).unwrap();
    for body in &mut bodies {
        assert!(matches!(poll_body_once(&runtime, body), Poll::Ready(None)));
    }
    assert_eq!(count(&probe.counts.source_drops), 2);
    assert_eq!(signals.completed().len(), 2);
    assert_eq!(active_endpoint(&endpoint), 2);
    drop(bodies);
    assert_eq!(active_endpoint(&endpoint), 2);
    drop(held_a);
    wait_for_active(&runtime, &endpoint, 1);
    assert_eq!(active_endpoint(&endpoint), 1);
    drop(held_b);
    wait_for_active(&runtime, &endpoint, 0);
    assert_eq!(active_endpoint(&endpoint), 0);
    drop(driver);
    drain(&runtime, guard);
    assert_eq!(bytes.used(), 0);
}

fn wait_for_active(
    runtime: &tokio::runtime::Runtime,
    endpoint: &Arc<crate::transport::EndpointState<super::super::super::state::AppContext>>,
    expected: usize,
) {
    runtime
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(7), async {
                while active_endpoint(endpoint) != expected {
                    tokio::task::yield_now().await;
                }
            })
            .await
        })
        .expect("bounded aggregate physical cleanup");
}

fn wait_for_count(runtime: &tokio::runtime::Runtime, counter: &AtomicUsize, expected: usize) {
    runtime
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(7), async {
                while count(counter) != expected {
                    tokio::task::yield_now().await;
                }
            })
            .await
        })
        .expect("bounded aggregate source cleanup");
}
