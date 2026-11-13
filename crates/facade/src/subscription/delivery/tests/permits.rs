use std::sync::{Arc, Barrier};

use super::constrained_settings;
use crate::subscription::delivery::{DeliveryResources, OperationKind, PermitError};

fn resources() -> DeliveryResources {
    let settings = constrained_settings(
        |limits| {
            limits.max_pending_operation_setups = 1;
            limits.max_active_subscriptions = 1;
        },
        |protocol| {
            protocol.max_websocket_connections = 1;
            protocol.max_pending_setups_per_connection = 1;
            protocol.max_active_operations_per_connection = 1;
        },
    );
    DeliveryResources::new(&settings).unwrap()
}

#[test]
fn endpoint_pending_transfers_atomically_to_active_subscription() {
    let resources = resources();
    let pending = resources.try_pending_operation().unwrap();
    assert_eq!(resources.snapshot().endpoint_pending, 1);
    assert!(matches!(
        resources.try_pending_operation(),
        Err(PermitError::Capacity)
    ));

    let active = pending.activate_subscription().unwrap();
    let snapshot = resources.snapshot();
    assert_eq!(snapshot.endpoint_pending, 0);
    assert_eq!(snapshot.endpoint_active_subscriptions, 1);
    drop(active);
    assert_eq!(resources.snapshot().endpoint_active_subscriptions, 0);
}

#[test]
fn revoked_generation_releases_id_but_retains_physical_quotas() {
    let resources = resources();
    let connection = resources.open_connection().unwrap();
    let pending = connection.begin(Arc::from("same-id")).unwrap();
    let old_generation = pending.generation();
    let active = pending.activate(OperationKind::Subscription).unwrap();
    let revoked = active.revoke();

    let replacement = connection.begin(Arc::from("same-id")).unwrap();
    assert!(replacement.generation() > old_generation);
    assert!(matches!(
        replacement.activate(OperationKind::Subscription),
        Err(PermitError::Capacity)
    ));
    let snapshot = connection.snapshot();
    assert_eq!(snapshot.connection_active, 1);
    assert_eq!(snapshot.endpoint_active_subscriptions, 1);

    drop(revoked);
    let replacement = connection.begin(Arc::from("same-id")).unwrap();
    let active = replacement.activate(OperationKind::Subscription).unwrap();
    assert_eq!(connection.snapshot().connection_active, 1);
    drop(active);
    assert_eq!(connection.snapshot().connection_active, 0);
}

#[test]
fn late_old_generation_release_cannot_touch_a_reused_logical_id() {
    let settings = constrained_settings(
        |limits| {
            limits.max_pending_operation_setups = 2;
            limits.max_active_subscriptions = 2;
        },
        |protocol| {
            protocol.max_websocket_connections = 1;
            protocol.max_pending_setups_per_connection = 2;
            protocol.max_active_operations_per_connection = 2;
        },
    );
    let resources = DeliveryResources::new(&settings).unwrap();
    let connection = resources.open_connection().unwrap();
    let old = connection
        .begin(Arc::from("same-id"))
        .unwrap()
        .activate(OperationKind::Subscription)
        .unwrap();
    let old_generation = old.generation();
    let revoked = old.revoke();
    let replacement = connection
        .begin(Arc::from("same-id"))
        .unwrap()
        .activate(OperationKind::Subscription)
        .unwrap();
    assert!(replacement.generation() > old_generation);

    drop(revoked);
    assert_eq!(connection.snapshot().connection_active, 1);
    assert_eq!(resources.snapshot().endpoint_active_subscriptions, 1);
    assert!(matches!(
        connection.begin(Arc::from("same-id")),
        Err(PermitError::DuplicateId)
    ));

    drop(replacement);
    assert_eq!(connection.snapshot().connection_active, 0);
    assert_eq!(resources.snapshot().endpoint_active_subscriptions, 0);
}

#[test]
fn old_physical_charge_keeps_the_connection_quota_after_logical_drop() {
    let resources = resources();
    let connection = resources.open_connection().unwrap();
    let active = connection
        .begin(Arc::from("query"))
        .unwrap()
        .activate(OperationKind::Query)
        .unwrap();
    let revoked = active.revoke();
    drop(connection);

    assert_eq!(resources.snapshot().endpoint_connections, 1);
    assert!(matches!(
        resources.open_connection(),
        Err(PermitError::Capacity)
    ));
    drop(revoked);
    assert_eq!(resources.snapshot().endpoint_connections, 0);
    let _new_connection = resources.open_connection().unwrap();
}

#[test]
fn connection_and_endpoint_pending_limits_choose_one_concurrent_winner() {
    let resources = resources();
    let connection = resources.open_connection().unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    let admission = connection.admission();
    for id in ["one", "two"] {
        let connection = admission.clone();
        let barrier = Arc::clone(&barrier);
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            connection.begin(Arc::from(id))
        }));
    }
    barrier.wait();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(connection.snapshot().connection_pending, 1);
    drop(results);
    assert_eq!(connection.snapshot().connection_pending, 0);
}

#[test]
fn every_operation_kind_uses_the_same_active_connection_quota() {
    let resources = resources();
    let connection = resources.open_connection().unwrap();
    for (id, kind) in [
        ("query", OperationKind::Query),
        ("mutation", OperationKind::Mutation),
        ("subscription", OperationKind::Subscription),
    ] {
        let active = connection
            .begin(Arc::from(id))
            .unwrap()
            .activate(kind)
            .unwrap();
        assert_eq!(connection.snapshot().connection_active, 1);
        drop(active);
        assert_eq!(connection.snapshot().connection_active, 0);
    }
}

#[test]
fn quarantine_capacity_overflow_is_rejected_instead_of_clamped() {
    let settings = constrained_settings(
        |_| {},
        |protocol| {
            protocol.max_websocket_connections = usize::MAX;
            protocol.max_active_operations_per_connection = 2;
        },
    );

    assert!(matches!(
        DeliveryResources::new(&settings),
        Err(PermitError::Invariant)
    ));
}
