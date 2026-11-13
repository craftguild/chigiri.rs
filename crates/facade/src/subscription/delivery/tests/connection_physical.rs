use std::sync::Arc;

use super::constrained_settings;
use crate::subscription::delivery::{DeliveryResources, OperationKind, PermitError};

fn resources(active: usize) -> DeliveryResources {
    let settings = constrained_settings(
        |limits| {
            limits.max_pending_operation_setups = 2;
            limits.max_active_subscriptions = active;
        },
        |protocol| {
            protocol.max_websocket_connections = 1;
            protocol.max_pending_setups_per_connection = 2;
            protocol.max_active_operations_per_connection = active;
        },
    );
    DeliveryResources::new(&settings).unwrap()
}

#[test]
fn every_kind_keeps_the_same_charge_until_cleanup_and_last_physical_drop() {
    for kind in [
        OperationKind::Query,
        OperationKind::Mutation,
        OperationKind::Subscription,
    ] {
        let resources = resources(1);
        let connection = resources.open_connection().unwrap();
        let active = connection
            .begin(Arc::from("same"))
            .unwrap()
            .activate(kind)
            .unwrap();
        let generation = active.generation();
        let frame = active.physical_lease().unwrap();
        let clone = frame.clone();
        assert_eq!(frame.generation(), generation);
        drop(active.revoke());

        let replacement = connection.begin(Arc::from("same")).unwrap();
        assert!(replacement.generation() > generation);
        assert!(matches!(
            replacement.activate(kind),
            Err(PermitError::Capacity)
        ));
        assert_eq!(connection.snapshot().connection_active, 1);
        assert_eq!(
            resources.snapshot().endpoint_active_subscriptions,
            usize::from(kind == OperationKind::Subscription)
        );
        drop(frame);
        assert_eq!(connection.snapshot().connection_active, 1);
        drop(clone);
        assert_eq!(connection.snapshot().connection_active, 0);
        assert_eq!(resources.snapshot().endpoint_active_subscriptions, 0);
        let replacement = connection
            .begin(Arc::from("same"))
            .unwrap()
            .activate(kind)
            .unwrap();
        drop(replacement);
        assert!(!connection.snapshot().corrupted);
    }
}

#[test]
fn late_frame_release_cannot_remove_or_release_the_reused_generation() {
    let resources = resources(2);
    let connection = resources.open_connection().unwrap();
    let old = connection
        .begin(Arc::from("same"))
        .unwrap()
        .activate(OperationKind::Subscription)
        .unwrap();
    let frame = old.physical_lease().unwrap();
    let generation = old.generation();
    drop(old);
    let current = connection
        .begin(Arc::from("same"))
        .unwrap()
        .activate(OperationKind::Subscription)
        .unwrap();
    assert!(current.generation() > generation);
    assert_eq!(connection.snapshot().connection_active, 2);
    drop(frame);
    assert_eq!(connection.snapshot().connection_active, 1);
    assert_eq!(resources.snapshot().endpoint_active_subscriptions, 1);
    assert!(matches!(
        connection.begin(Arc::from("same")),
        Err(PermitError::DuplicateId)
    ));
    drop(current);
    assert_eq!(connection.snapshot().connection_active, 0);
    assert!(!connection.snapshot().corrupted);
}

#[test]
fn a_frame_keeps_connection_capacity_after_every_logical_owner_is_gone() {
    let resources = resources(1);
    let connection = resources.open_connection().unwrap();
    let admission = connection.admission();
    let active = connection
        .begin(Arc::from("operation"))
        .unwrap()
        .activate(OperationKind::Query)
        .unwrap();
    let frame = active.physical_lease().unwrap();
    drop(active);
    drop(connection);
    assert_eq!(resources.snapshot().endpoint_connections, 1);
    assert!(matches!(
        resources.open_connection(),
        Err(PermitError::Capacity)
    ));
    assert!(matches!(
        admission.begin(Arc::from("new")),
        Err(PermitError::StaleGeneration)
    ));
    drop(frame);
    assert_eq!(resources.snapshot().endpoint_connections, 0);
    let connection = resources.open_connection().unwrap();
    assert_eq!(connection.snapshot().connection_active, 0);
}

#[test]
fn a_logically_closed_connection_cannot_issue_a_new_physical_operation_lease() {
    let resources = resources(1);
    let connection = resources.open_connection().unwrap();
    let active = connection
        .begin(Arc::from("operation"))
        .unwrap()
        .activate(OperationKind::Mutation)
        .unwrap();
    let revoked = connection.revoke();
    assert!(matches!(
        active.physical_lease(),
        Err(PermitError::StaleGeneration)
    ));
    drop(active);
    drop(revoked);
    assert_eq!(resources.snapshot().endpoint_connections, 0);
}
