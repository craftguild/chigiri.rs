//! Focused ownership tests for the bridge-facing wrapper.

mod connection_cleanup;
mod coordinator;
mod operation_physical;
mod physical;
mod support;

use std::future::pending;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use super::super::settings::{
    SubscriptionLimits, SubscriptionProtocolSettings, ValidatedSubscriptionSettings,
};
use super::*;

fn owner() -> BridgeResourceOwner {
    let limits = SubscriptionLimits {
        max_active_subscriptions: 2,
        max_pending_operation_setups: 2,
        ..SubscriptionLimits::default()
    };
    let protocol = SubscriptionProtocolSettings {
        max_active_operations_per_connection: 2,
        max_websocket_connections: 1,
        max_pending_setups_per_connection: 2,
    };
    BridgeResourceOwner::new(&ValidatedSubscriptionSettings::new(limits, protocol)).unwrap()
}

#[test]
fn http_uses_the_unified_pending_active_and_terminal_owners() {
    let owner = owner();
    let pending = owner.begin_http_subscription().unwrap();
    let generation = pending.generation();
    assert_eq!(owner.delivery().snapshot().endpoint_pending, 1);
    assert_eq!(
        pending.activate(BridgeOperationKind::Query).err(),
        Some(BridgeResourceError::InvalidOperationKind)
    );
    assert_eq!(owner.delivery().snapshot().endpoint_pending, 0);

    let active = owner
        .begin_http_subscription()
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    assert!(active.generation() > generation);
    let cancellation = active.cancellation();
    let emission = active.emission_lease();
    assert!(emission.can_emit());
    let terminal = active.revoke().unwrap();
    assert!(cancellation.is_cancelled());
    assert!(!emission.can_emit());
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 1);
    drop(terminal);
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 0);
}

#[test]
fn connection_charges_all_kinds_and_old_release_cannot_touch_reuse() {
    let owner = owner();
    let connection = owner.open_connection().unwrap();
    for (id, kind) in [
        ("query", BridgeOperationKind::Query),
        ("mutation", BridgeOperationKind::Mutation),
        ("subscription", BridgeOperationKind::Subscription),
    ] {
        let active = connection
            .begin(Arc::from(id))
            .unwrap()
            .activate(kind)
            .unwrap();
        let terminal = active.revoke().unwrap();
        let replacement = connection
            .begin(Arc::from(id))
            .unwrap()
            .activate(kind)
            .unwrap();
        drop(terminal);
        assert_eq!(connection.snapshot().connection_active, 1);
        assert_eq!(
            connection.begin(Arc::from(id)).err(),
            Some(BridgeResourceError::DuplicateId)
        );
        drop(replacement);
        assert_eq!(connection.snapshot().connection_active, 0);
    }
}

#[test]
fn quarantine_completion_preserves_move_only_charges() {
    let owner = owner();
    let cleanup = owner.cleanup_driver();
    let first = owner
        .begin_http_subscription()
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap()
        .revoke()
        .unwrap();
    first.quarantine(DetachedCleanup::new(pending())).unwrap();
    let second = owner
        .begin_http_subscription()
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap()
        .revoke()
        .unwrap();
    second.quarantine(DetachedCleanup::new(async {})).unwrap();
    assert_eq!(cleanup.pending(), Ok(2));
    let third = owner.begin_http_subscription().unwrap();
    assert_eq!(
        third.activate(BridgeOperationKind::Subscription).err(),
        Some(BridgeResourceError::Capacity)
    );

    let mut context = Context::from_waker(Waker::noop());
    assert!(matches!(cleanup.poll_one(&mut context), Poll::Pending));
    assert_eq!(cleanup.poll_one(&mut context), Poll::Ready(Ok(true)));
    assert_eq!(cleanup.pending(), Ok(1));
}

#[test]
fn cleanup_reservation_failure_rolls_back_before_an_owner_escapes() {
    use super::super::delivery::{DeliveryResources, QuarantineRegistry};

    let settings = ValidatedSubscriptionSettings::new(
        SubscriptionLimits::default(),
        SubscriptionProtocolSettings::default(),
    );
    let resources = DeliveryResources::new(&settings).unwrap();
    let pending = resources.try_pending_operation().unwrap();
    let generation = pending.generation();
    assert_eq!(
        super::admission::reserve_ticket(&QuarantineRegistry::new(0), generation).err(),
        Some(BridgeResourceError::Capacity)
    );
    drop(pending);
    assert_eq!(resources.snapshot().endpoint_pending, 0);
}

#[test]
fn pending_revocation_releases_id_before_old_physical_completion() {
    let owner = owner();
    let cleanup = owner.cleanup_driver();
    let connection = owner.open_connection().unwrap();
    let terminal = connection
        .begin(Arc::from("same"))
        .unwrap()
        .revoke()
        .unwrap();
    assert_eq!(connection.snapshot().connection_pending, 1);
    terminal.quarantine(DetachedCleanup::new(async {})).unwrap();

    let replacement = connection.begin(Arc::from("same")).unwrap();
    assert_eq!(connection.snapshot().connection_pending, 2);
    let mut context = Context::from_waker(Waker::noop());
    assert_eq!(cleanup.poll_one(&mut context), Poll::Ready(Ok(true)));
    assert_eq!(connection.snapshot().connection_pending, 1);
    assert_eq!(
        connection.begin(Arc::from("same")).err(),
        Some(BridgeResourceError::DuplicateId)
    );
    drop(replacement);
    assert_eq!(connection.snapshot().connection_pending, 0);
}

#[test]
fn quarantine_bound_includes_every_coexisting_pending_and_active_owner() {
    let owner = owner();
    let cleanup = owner.cleanup_driver();
    let connection = owner.open_connection().unwrap();
    let active_query = connection
        .begin(Arc::from("query"))
        .unwrap()
        .activate(BridgeOperationKind::Query)
        .unwrap();
    let active_mutation = connection
        .begin(Arc::from("mutation"))
        .unwrap()
        .activate(BridgeOperationKind::Mutation)
        .unwrap();
    let active_http_one = owner
        .begin_http_subscription()
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    let active_http_two = owner
        .begin_http_subscription()
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    let pending_one = connection.begin(Arc::from("pending-one")).unwrap();
    let pending_two = connection.begin(Arc::from("pending-two")).unwrap();

    assert_eq!(
        connection.begin(Arc::from("impossible")).err(),
        Some(BridgeResourceError::Capacity)
    );
    let terminals = [
        active_query.revoke().unwrap(),
        active_mutation.revoke().unwrap(),
        active_http_one.revoke().unwrap(),
        active_http_two.revoke().unwrap(),
        pending_one.revoke().unwrap(),
        pending_two.revoke().unwrap(),
    ];
    assert_eq!(owner.delivery().snapshot().endpoint_pending, 2);
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 2);
    assert_eq!(connection.snapshot().connection_active, 2);
    assert_eq!(connection.snapshot().connection_pending, 2);
    for terminal in terminals {
        terminal.quarantine(DetachedCleanup::new(async {})).unwrap();
    }
    assert_eq!(cleanup.pending(), Ok(6));

    let mut context = Context::from_waker(Waker::noop());
    for remaining in (0..6).rev() {
        assert_eq!(cleanup.poll_one(&mut context), Poll::Ready(Ok(true)));
        assert_eq!(cleanup.pending(), Ok(remaining));
    }
    assert_eq!(owner.delivery().snapshot().endpoint_pending, 0);
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 0);
    assert_eq!(connection.snapshot().connection_pending, 0);
    assert_eq!(connection.snapshot().connection_active, 0);
}
