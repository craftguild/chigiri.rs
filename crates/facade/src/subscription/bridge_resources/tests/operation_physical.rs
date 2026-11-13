use std::sync::Arc;
use std::task::Poll;

use super::super::{BridgeCleanupOutcome, BridgeOperationKind, BridgeResourceError};
use super::owner;
use super::support::{CleanupProbe, ManualDeadline, poll_cleanup_driver, poll_once};

#[test]
fn joined_cleanup_and_writer_share_one_original_operation_charge() {
    for kind in [
        BridgeOperationKind::Query,
        BridgeOperationKind::Mutation,
        BridgeOperationKind::Subscription,
    ] {
        let owner = owner();
        let connection = owner.open_connection().unwrap();
        let active = connection
            .begin(Arc::from("id"))
            .unwrap()
            .activate(kind)
            .unwrap();
        let frame = active.websocket_physical_lease().unwrap();
        let clone = frame.clone();
        assert_eq!(frame.generation(), active.generation());
        let emission = active.emission_lease();
        let terminal = active.revoke().unwrap();
        assert!(!emission.can_emit());
        let deadline = ManualDeadline::default();
        let probe = CleanupProbe::ready();
        let mut cleanup = Box::pin(terminal.coordinate(probe.cleanup(), &deadline));
        assert_eq!(
            poll_once(cleanup.as_mut()),
            Poll::Ready(BridgeCleanupOutcome::Completed)
        );
        drop(cleanup);
        assert_eq!(connection.snapshot().connection_active, 1);
        drop(frame);
        assert_eq!(connection.snapshot().connection_active, 1);
        drop(clone);
        assert_eq!(connection.snapshot().connection_active, 0);
        assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 0);
        assert_eq!(probe.drops(), 1);
    }
}

#[test]
fn physical_drop_first_cannot_release_a_live_or_quarantined_worker_charge() {
    let owner = owner();
    let connection = owner.open_connection().unwrap();
    let active = connection
        .begin(Arc::from("id"))
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    drop(active.websocket_physical_lease().unwrap());
    assert_eq!(connection.snapshot().connection_active, 1);
    let probe = CleanupProbe::default();
    let deadline = ManualDeadline::default();
    deadline.expire();
    let driver = owner.cleanup_driver();
    let mut cleanup = Box::pin(
        active
            .revoke()
            .unwrap()
            .coordinate(probe.cleanup(), &deadline),
    );
    assert_eq!(
        poll_once(cleanup.as_mut()),
        Poll::Ready(BridgeCleanupOutcome::Quarantined)
    );
    drop(cleanup);
    assert_eq!(driver.pending(), Ok(1));
    assert_eq!(connection.snapshot().connection_active, 1);
    probe.release();
    assert_eq!(poll_cleanup_driver(&driver), Poll::Ready(Ok(true)));
    assert_eq!(connection.snapshot().connection_active, 0);
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 0);
}

#[test]
fn quarantine_join_does_not_release_a_later_physical_frame_clone() {
    let owner = owner();
    let connection = owner.open_connection().unwrap();
    let active = connection
        .begin(Arc::from("id"))
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    let frame = active.websocket_physical_lease().unwrap();
    let probe = CleanupProbe::default();
    let deadline = ManualDeadline::default();
    deadline.expire();
    let driver = owner.cleanup_driver();
    let mut cleanup = Box::pin(
        active
            .revoke()
            .unwrap()
            .coordinate(probe.cleanup(), &deadline),
    );
    assert_eq!(
        poll_once(cleanup.as_mut()),
        Poll::Ready(BridgeCleanupOutcome::Quarantined)
    );
    drop(cleanup);
    let replacement = connection
        .begin(Arc::from("id"))
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    assert_ne!(frame.generation(), replacement.generation());
    assert_eq!(connection.snapshot().connection_active, 2);
    probe.release();
    assert_eq!(poll_cleanup_driver(&driver), Poll::Ready(Ok(true)));
    assert_eq!(connection.snapshot().connection_active, 2);
    drop(frame);
    assert_eq!(connection.snapshot().connection_active, 1);
    assert!(replacement.emission_lease().can_emit());
    assert_eq!(
        connection.begin(Arc::from("id")).err(),
        Some(BridgeResourceError::DuplicateId)
    );
    drop(replacement);
    assert_eq!(connection.snapshot().connection_active, 0);
}

#[test]
fn http_operations_cannot_issue_websocket_physical_authority() {
    let owner = owner();
    let active = owner
        .begin_http_subscription()
        .unwrap()
        .activate(BridgeOperationKind::Subscription)
        .unwrap();
    assert_eq!(
        active.websocket_physical_lease().err(),
        Some(BridgeResourceError::InvalidOperationKind)
    );
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 1);
    drop(active);
    assert_eq!(owner.delivery().snapshot().endpoint_active_subscriptions, 0);
}
