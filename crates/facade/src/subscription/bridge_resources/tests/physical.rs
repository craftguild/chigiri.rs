use std::sync::Arc;
use std::task::Poll;

use super::super::{BridgeCleanupOutcome, BridgeOperationKind, BridgeResourceError};
use super::support::{CleanupProbe, ManualDeadline, poll_cleanup_driver, poll_once};
use crate::subscription::bridge_resources::{
    BridgeResourceOwner, HttpSubscriptionPhysicalLease, TerminalBridgeOperation,
};
use crate::subscription::settings::{
    SubscriptionLimits, SubscriptionProtocolSettings, ValidatedSubscriptionSettings,
};

fn owner() -> BridgeResourceOwner {
    let limits = SubscriptionLimits {
        max_active_subscriptions: 1,
        max_pending_operation_setups: 2,
        ..SubscriptionLimits::default()
    };
    let protocol = SubscriptionProtocolSettings {
        max_active_operations_per_connection: 1,
        max_websocket_connections: 1,
        max_pending_setups_per_connection: 1,
    };
    BridgeResourceOwner::new(&ValidatedSubscriptionSettings::new(limits, protocol)).unwrap()
}

fn active_http(
    owner: &BridgeResourceOwner,
) -> (
    super::super::ActiveBridgeOperation,
    HttpSubscriptionPhysicalLease,
) {
    let active = owner
        .begin_http_subscription()
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    let lease = active.http_physical_lease().unwrap();
    assert_eq!(lease.generation(), active.generation());
    (active, lease)
}

fn complete_source_cleanup(terminal: TerminalBridgeOperation) {
    let deadlines = ManualDeadline::default();
    let probe = CleanupProbe::ready();
    let mut coordinator = Box::pin(terminal.coordinate(probe.cleanup(), &deadlines));
    assert_eq!(
        poll_once(coordinator.as_mut()),
        Poll::Ready(BridgeCleanupOutcome::Completed)
    );
    assert_eq!(probe.drops(), 1);
}

fn assert_active_capacity_denied(owner: &BridgeResourceOwner) {
    let pending = owner.begin_http_subscription().unwrap();
    assert_eq!(
        pending.activate(BridgeOperationKind::Subscription).err(),
        Some(BridgeResourceError::Capacity)
    );
}

#[test]
fn source_cleanup_first_retains_quota_until_the_last_writer_clone_drops() {
    let owner = owner();
    let (active, writer) = active_http(&owner);
    let second_writer = writer.clone();
    complete_source_cleanup(active.revoke().unwrap());

    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 1);
    assert_active_capacity_denied(&owner);
    drop(writer);
    assert_active_capacity_denied(&owner);
    drop(second_writer);
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 0);

    let replacement = owner
        .begin_http_subscription()
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    drop(replacement);
}

#[test]
fn writer_first_retains_quota_until_source_cleanup_finishes() {
    let owner = owner();
    let (active, writer) = active_http(&owner);
    drop(writer);
    assert_active_capacity_denied(&owner);

    complete_source_cleanup(active.revoke().unwrap());
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 0);
}

#[test]
fn quarantine_and_writer_share_the_same_single_physical_charge() {
    let owner = owner();
    let cleanup = owner.cleanup_driver();
    let (active, writer) = active_http(&owner);
    let deadlines = ManualDeadline::default();
    deadlines.expire();
    let probe = CleanupProbe::default();
    let mut coordinator = Box::pin(
        active
            .revoke()
            .unwrap()
            .coordinate(probe.cleanup(), &deadlines),
    );
    assert_eq!(
        poll_once(coordinator.as_mut()),
        Poll::Ready(BridgeCleanupOutcome::Quarantined)
    );
    drop(coordinator);

    assert_eq!(cleanup.pending(), Ok(1));
    assert_active_capacity_denied(&owner);
    probe.release();
    assert_eq!(poll_cleanup_driver(&cleanup), Poll::Ready(Ok(true)));
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 1);
    assert_active_capacity_denied(&owner);
    drop(writer);
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 0);
}

#[test]
fn physical_leases_are_isolated_between_endpoint_owners() {
    let first = owner();
    let second = owner();
    let (active, writer) = active_http(&first);
    complete_source_cleanup(active.revoke().unwrap());

    assert_active_capacity_denied(&first);
    let second_active = second
        .begin_http_subscription()
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    assert_eq!(
        second.delivery().snapshot().endpoint_active_subscriptions,
        1
    );
    drop(second_active);
    assert_eq!(
        second.delivery().snapshot().endpoint_active_subscriptions,
        0
    );
    assert_eq!(first.delivery().snapshot().endpoint_active_subscriptions, 1);
    drop(writer);
    assert_eq!(first.delivery().snapshot().endpoint_active_subscriptions, 0);
}

#[test]
fn connection_operations_cannot_issue_an_http_physical_lease() {
    let owner = owner();
    let connection = owner.open_connection().unwrap();
    let active = connection
        .begin(Arc::from("subscription"))
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    assert_eq!(
        active.http_physical_lease().err(),
        Some(BridgeResourceError::InvalidOperationKind)
    );
    assert_eq!(connection.snapshot().connection_active, 1);
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 1);
    drop(active);
    assert_eq!(connection.snapshot().connection_active, 0);
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 0);
}
