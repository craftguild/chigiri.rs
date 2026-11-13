//! Aggregate driver Wakers are cloned and disposed outside RegistryState.

use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use chigiri_http::SubscriptionTransport;

mod probe;

use super::super::super::state::{SourceStep, State};
use super::super::support as worker;
use super::aggregate_support::{AggregateSupervision, attach_aggregate};
use super::support::active_endpoint;
use crate::subscription::bridge_resources::BridgeResourceOwner;
use crate::subscription::settings::{
    SubscriptionLimits, SubscriptionProtocolSettings, ValidatedSubscriptionSettings,
};
use crate::transport::streaming::supervisor::{HttpStreamingError, HttpStreamingRegistration};
use probe::{ProbeCounts, probing_waker};

const PROFILES: [SubscriptionTransport; 2] = [
    SubscriptionTransport::ServerSentEvents,
    SubscriptionTransport::Multipart,
];

#[test]
fn pending_transfer_preserves_two_real_entry_owners() {
    let runtime = worker::runtime();
    let probe = State::new(vec![SourceStep::Pending]);
    let (mut live, _) =
        attach_aggregate(&runtime, probe, &PROFILES, Duration::from_secs(60), false);
    assert_eq!(live.driver.registry_ownership_for_test(), (2, 0));

    let mut context = Context::from_waker(std::task::Waker::noop());
    {
        let _entered = runtime.enter();
        assert!(live.driver.as_mut().poll(&mut context).is_pending());
    }

    assert_eq!(live.driver.registry_ownership_for_test(), (0, 2));
    live.registration.close();
    worker::run(&runtime, live.driver.as_mut()).unwrap();
    finish(&runtime, live);
}

#[test]
fn open_empty_registry_replaces_then_wakes_without_lock_reentry() {
    let settings = ValidatedSubscriptionSettings::new(
        SubscriptionLimits::default(),
        SubscriptionProtocolSettings::default(),
    );
    let owner = BridgeResourceOwner::new(&settings).unwrap();
    let (registration, driver) =
        HttpStreamingRegistration::new(owner.delivery().byte_owner(), 1).unwrap();
    let mut driver = Box::pin(driver);

    let counts = ProbeCounts::new();
    let target = driver.registry_lock_probe_for_test();
    let old = probing_waker(Arc::clone(&target), counts.clone());
    driver.set_registry_waker_for_test(old.into_waker());
    let replacement = probing_waker(target, counts.clone());
    let mut context = Context::from_waker(replacement.waker());
    assert!(driver.as_mut().poll(&mut context).is_pending());
    assert_eq!(replacement.strong_count(), 2);
    assert_eq!(counts.values(), (1, 0, 0));
    registration.close();
    assert_eq!(replacement.strong_count(), 1);
    assert_eq!(counts.values(), (1, 1, 0));
    drop(replacement);
    assert_eq!(counts.values(), (2, 1, 0));
    drop(driver);
    drop(registration);
}

#[test]
fn poisoned_registry_drops_the_unused_clone_after_the_guard() {
    let runtime = worker::runtime();
    let probe = State::new(vec![SourceStep::Pending]);
    let (mut live, _) =
        attach_aggregate(&runtime, probe, &PROFILES, Duration::from_secs(60), false);
    assert_eq!(live.driver.registry_ownership_for_test(), (2, 0));
    let counts = ProbeCounts::new();
    let replacement = probing_waker(live.driver.registry_lock_probe_for_test(), counts.clone());
    live.driver.poison_registry_for_test();
    let mut context = Context::from_waker(replacement.waker());
    assert_eq!(
        live.driver.as_mut().poll(&mut context),
        Poll::Ready(Err(HttpStreamingError::Invariant))
    );

    assert_eq!(replacement.strong_count(), 1);
    assert_eq!(counts.values(), (0, 0, 0));
    drop(replacement);
    assert_eq!(counts.values(), (1, 0, 0));
    finish(&runtime, live);
}

#[test]
fn slot_conflict_fails_closed_without_disposing_wakers_under_lock() {
    let runtime = worker::runtime();
    let probe = State::new(vec![SourceStep::Pending]);
    let (mut live, _) =
        attach_aggregate(&runtime, probe, &PROFILES, Duration::from_secs(60), false);
    let mut first = Context::from_waker(std::task::Waker::noop());
    {
        let _entered = runtime.enter();
        assert!(live.driver.as_mut().poll(&mut first).is_pending());
    }
    assert_eq!(live.driver.registry_ownership_for_test(), (0, 2));
    live.driver
        .as_mut()
        .get_mut()
        .inject_registry_conflict_for_test();
    assert_eq!(live.driver.registry_ownership_for_test(), (1, 1));

    let counts = ProbeCounts::new();
    let target = live.driver.registry_lock_probe_for_test();
    let old = probing_waker(Arc::clone(&target), counts.clone());
    live.driver.set_registry_waker_for_test(old.into_waker());
    let replacement = probing_waker(target, counts.clone());
    let mut context = Context::from_waker(replacement.waker());
    {
        let _entered = runtime.enter();
        assert_eq!(
            live.driver.as_mut().poll(&mut context),
            Poll::Ready(Err(HttpStreamingError::Invariant))
        );
    }

    assert_eq!(replacement.strong_count(), 1);
    assert_eq!(counts.values(), (1, 1, 0));
    drop(replacement);
    assert_eq!(counts.values(), (2, 1, 0));
    finish(&runtime, live);
}

#[test]
fn closed_empty_registry_replaces_then_wakes_without_lock_reentry() {
    let settings = ValidatedSubscriptionSettings::new(
        SubscriptionLimits::default(),
        SubscriptionProtocolSettings::default(),
    );
    let owner = BridgeResourceOwner::new(&settings).unwrap();
    let (registration, driver) =
        HttpStreamingRegistration::new(owner.delivery().byte_owner(), 1).unwrap();
    let mut driver = Box::pin(driver);
    registration.close();

    let counts = ProbeCounts::new();
    let target = driver.registry_lock_probe_for_test();
    let old = probing_waker(Arc::clone(&target), counts.clone());
    driver.set_registry_waker_for_test(old.into_waker());
    let replacement = probing_waker(target, counts.clone());
    let mut context = Context::from_waker(replacement.waker());
    assert_eq!(driver.as_mut().poll(&mut context), Poll::Ready(Ok(())));
    assert_eq!(replacement.strong_count(), 2);
    assert_eq!(counts.values(), (1, 0, 0));
    drop(driver);
    assert_eq!(replacement.strong_count(), 1);
    assert_eq!(counts.values(), (1, 1, 0));
    drop(replacement);
    assert_eq!(counts.values(), (2, 1, 0));
    drop(registration);
}

fn finish(runtime: &tokio::runtime::Runtime, live: AggregateSupervision) {
    let AggregateSupervision {
        endpoint,
        registration,
        driver,
        bodies,
        guard,
    } = live;
    drop(registration);
    drop(driver);
    drop(bodies);
    worker::drain(runtime, guard);
    assert_eq!(active_endpoint(&endpoint), 0);
}
