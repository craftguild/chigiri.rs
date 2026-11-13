use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use super::super::{BridgeCleanupOutcome, BridgeOperationKind, DetachedCleanup};
use super::owner;
use super::support::{CleanupProbe, ManualDeadline, poll_cleanup_driver, poll_once};
use crate::subscription::settings::{
    SubscriptionLimits, SubscriptionProtocolSettings, ValidatedSubscriptionSettings,
};

fn terminal(owner: &super::super::BridgeResourceOwner) -> super::super::TerminalBridgeOperation {
    owner
        .begin_http_subscription()
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap()
        .revoke()
        .unwrap()
}

fn single_owner() -> super::super::BridgeResourceOwner {
    let limits = SubscriptionLimits {
        max_active_subscriptions: 1,
        max_pending_operation_setups: 1,
        ..SubscriptionLimits::default()
    };
    super::super::BridgeResourceOwner::new(&ValidatedSubscriptionSettings::new(
        limits,
        SubscriptionProtocolSettings::default(),
    ))
    .unwrap()
}

struct ReadyDropPanic(Arc<AtomicUsize>);

impl Future for ReadyDropPanic {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(())
    }
}

impl Drop for ReadyDropPanic {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::AcqRel);
        panic!("cleanup destructor panic")
    }
}

#[test]
fn ready_cleanup_releases_the_reserved_slot_and_physical_charge() {
    let owner = owner();
    let deadlines = ManualDeadline::default();
    let probe = CleanupProbe::ready();
    let mut coordinator = pin!(terminal(&owner).coordinate(probe.cleanup(), &deadlines));

    assert_eq!(
        poll_once(coordinator.as_mut()),
        Poll::Ready(BridgeCleanupOutcome::Completed)
    );
    assert_eq!(probe.polls(), 1);
    assert_eq!(probe.drops(), 1);
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 0);
    assert_eq!(owner.cleanup_driver().pending(), Ok(0));
    assert_eq!(deadlines.requested(), vec![Duration::from_secs(5)]);
}

#[test]
fn timeout_precedes_cleanup_in_an_exact_race_and_retains_capacity() {
    let owner = owner();
    let cleanup = owner.cleanup_driver();
    let deadlines = ManualDeadline::default();
    let probe = CleanupProbe::ready();
    deadlines.expire();
    let mut coordinator = pin!(terminal(&owner).coordinate(probe.cleanup(), &deadlines));

    assert_eq!(
        poll_once(coordinator.as_mut()),
        Poll::Ready(BridgeCleanupOutcome::Quarantined)
    );
    assert_eq!(probe.polls(), 0);
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 1);
    assert_eq!(cleanup.pending(), Ok(1));
    assert_eq!(poll_cleanup_driver(&cleanup), Poll::Ready(Ok(true)));
    assert_eq!(probe.polls(), 1);
    assert_eq!(probe.drops(), 1);
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 0);
}

#[test]
fn dropping_an_unpolled_or_pending_coordinator_uses_the_same_quarantine_path() {
    for poll_before_drop in [false, true] {
        let owner = owner();
        let cleanup = owner.cleanup_driver();
        let deadlines = ManualDeadline::default();
        let probe = CleanupProbe::default();
        let mut coordinator = Box::pin(terminal(&owner).coordinate(probe.cleanup(), &deadlines));
        if poll_before_drop {
            assert_eq!(poll_once(coordinator.as_mut()), Poll::Pending);
            assert_eq!(probe.polls(), 1);
        }

        drop(coordinator);
        assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 1);
        assert_eq!(cleanup.pending(), Ok(1));
        probe.release();
        assert_eq!(poll_cleanup_driver(&cleanup), Poll::Ready(Ok(true)));
        assert_eq!(probe.drops(), 1);
        assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 0);
    }
}

#[test]
fn cancellation_resistant_cleanup_denies_capacity_until_late_completion() {
    let owner = single_owner();
    let cleanup = owner.cleanup_driver();
    let deadlines = ManualDeadline::default();
    let probe = CleanupProbe::default();
    deadlines.expire();
    let mut coordinator = pin!(terminal(&owner).coordinate(probe.cleanup(), &deadlines));
    assert_eq!(
        poll_once(coordinator.as_mut()),
        Poll::Ready(BridgeCleanupOutcome::Quarantined)
    );

    let pending = owner.begin_http_subscription().unwrap();
    assert!(pending.activate(BridgeOperationKind::Subscription).is_err());
    assert_eq!(poll_cleanup_driver(&cleanup), Poll::Pending);
    probe.release();
    assert_eq!(poll_cleanup_driver(&cleanup), Poll::Ready(Ok(true)));
    assert_eq!(probe.drops(), 1);

    let active = owner
        .begin_http_subscription()
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    drop(active);
}

#[test]
fn ready_cleanup_destructor_panic_quarantines_physical_charge_exactly_once() {
    let owner = single_owner();
    let cleanup = owner.cleanup_driver();
    let deadlines = ManualDeadline::default();
    let future_drops = Arc::new(AtomicUsize::new(0));
    let detached = DetachedCleanup::new(ReadyDropPanic(Arc::clone(&future_drops)));
    let mut coordinator = Box::pin(terminal(&owner).coordinate(detached, &deadlines));

    let panic = catch_unwind(AssertUnwindSafe(|| poll_once(coordinator.as_mut())));
    assert!(panic.is_err());
    assert_eq!(future_drops.load(Ordering::Acquire), 1);
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 1);
    assert_eq!(cleanup.pending(), Ok(1));
    let pending = owner.begin_http_subscription().unwrap();
    assert!(pending.activate(BridgeOperationKind::Subscription).is_err());

    assert!(catch_unwind(AssertUnwindSafe(|| drop(coordinator))).is_ok());
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 1);
    assert_eq!(poll_cleanup_driver(&cleanup), Poll::Ready(Ok(true)));
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 0);
    assert_eq!(cleanup.pending(), Ok(0));
    assert_eq!(poll_cleanup_driver(&cleanup), Poll::Ready(Ok(false)));
    assert_eq!(future_drops.load(Ordering::Acquire), 1);
    let active = owner
        .begin_http_subscription()
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    drop(active);
}
