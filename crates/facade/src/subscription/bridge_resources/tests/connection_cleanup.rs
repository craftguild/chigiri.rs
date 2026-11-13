use std::sync::Arc;
use std::task::Poll;

use super::super::{
    BridgeCleanupOutcome, BridgeConnection, BridgeOperationKind, BridgeResourceError,
};
use super::owner;
use super::support::{CleanupProbe, ManualDeadline, poll_cleanup_driver, poll_once};
use crate::subscription::delivery::QuarantineRegistry;

#[test]
fn ready_connection_cleanup_closes_every_weak_admission_view() {
    let owner = owner();
    let connection = owner.open_connection().unwrap();
    let first = connection.admission();
    let second = first.clone();
    let terminal = connection.revoke().unwrap();
    assert_eq!(
        first.begin(Arc::from("closed")).err(),
        Some(BridgeResourceError::StaleGeneration)
    );
    assert_eq!(
        second.begin(Arc::from("also-closed")).err(),
        Some(BridgeResourceError::StaleGeneration)
    );

    let deadlines = ManualDeadline::default();
    let probe = CleanupProbe::ready();
    let mut coordinator = Box::pin(terminal.coordinate(probe.cleanup(), &deadlines));
    assert_eq!(
        poll_once(coordinator.as_mut()),
        Poll::Ready(BridgeCleanupOutcome::Completed)
    );
    assert_eq!(owner.delivery().snapshot().endpoint_connections, 0);
    assert!(owner.open_connection().is_ok());
}

#[test]
fn connection_timeout_and_coordinator_drop_retain_the_connection_permit() {
    for drop_coordinator in [false, true] {
        let owner = owner();
        let cleanup = owner.cleanup_driver();
        let deadlines = ManualDeadline::default();
        let probe = CleanupProbe::default();
        let terminal = owner.open_connection().unwrap().revoke().unwrap();
        let mut coordinator = Box::pin(terminal.coordinate(probe.cleanup(), &deadlines));
        if drop_coordinator {
            drop(coordinator);
        } else {
            deadlines.expire();
            assert_eq!(
                poll_once(coordinator.as_mut()),
                Poll::Ready(BridgeCleanupOutcome::Quarantined)
            );
        }
        assert_eq!(owner.delivery().snapshot().endpoint_connections, 1);
        assert_eq!(
            owner.open_connection().err(),
            Some(BridgeResourceError::Capacity)
        );
        probe.release();
        assert_eq!(poll_cleanup_driver(&cleanup), Poll::Ready(Ok(true)));
        assert_eq!(probe.drops(), 1);
        assert_eq!(owner.delivery().snapshot().endpoint_connections, 0);
    }
}

#[test]
fn dropping_a_pending_connection_coordinator_uses_the_quarantine_path() {
    let owner = owner();
    let cleanup = owner.cleanup_driver();
    let deadlines = ManualDeadline::default();
    let probe = CleanupProbe::default();
    let terminal = owner.open_connection().unwrap().revoke().unwrap();
    let mut coordinator = Box::pin(terminal.coordinate(probe.cleanup(), &deadlines));
    assert_eq!(poll_once(coordinator.as_mut()), Poll::Pending);
    assert_eq!(probe.polls(), 1);

    drop(coordinator);
    assert_eq!(owner.delivery().snapshot().endpoint_connections, 1);
    assert_eq!(cleanup.pending(), Ok(1));
    probe.release();
    assert_eq!(poll_cleanup_driver(&cleanup), Poll::Ready(Ok(true)));
    assert_eq!(probe.drops(), 1);
    assert_eq!(owner.delivery().snapshot().endpoint_connections, 0);
}

#[test]
fn connection_timeout_wins_an_exact_ready_race() {
    let owner = owner();
    let cleanup = owner.cleanup_driver();
    let deadlines = ManualDeadline::default();
    let probe = CleanupProbe::ready();
    let terminal = owner.open_connection().unwrap().revoke().unwrap();
    deadlines.expire();
    let mut coordinator = Box::pin(terminal.coordinate(probe.cleanup(), &deadlines));

    assert_eq!(
        poll_once(coordinator.as_mut()),
        Poll::Ready(BridgeCleanupOutcome::Quarantined)
    );
    assert_eq!(probe.polls(), 0);
    assert_eq!(owner.delivery().snapshot().endpoint_connections, 1);
    assert_eq!(poll_cleanup_driver(&cleanup), Poll::Ready(Ok(true)));
    assert_eq!(probe.polls(), 1);
    assert_eq!(probe.drops(), 1);
    assert_eq!(owner.delivery().snapshot().endpoint_connections, 0);
}

#[test]
fn connection_reservation_failure_rolls_back_before_a_view_escapes() {
    let owner = owner();
    let physical = owner.delivery().open_connection().unwrap();
    assert_eq!(owner.delivery().snapshot().endpoint_connections, 1);

    let result = BridgeConnection::new(physical, QuarantineRegistry::new(0));
    assert_eq!(result.err(), Some(BridgeResourceError::Capacity));
    assert_eq!(owner.delivery().snapshot().endpoint_connections, 0);
}

#[test]
fn quarantined_connection_cleanup_retains_an_accepted_frame_charge() {
    let owner = owner();
    let bytes = owner.delivery().byte_owner();
    let charge = bytes
        .try_reserve(8)
        .unwrap()
        .commit(8)
        .unwrap()
        .into_in_flight();
    assert_eq!(bytes.used(), 8);
    let cleanup = owner.cleanup_driver();
    let deadlines = ManualDeadline::default();
    deadlines.expire();
    let probe = CleanupProbe::default();
    let terminal = owner.open_connection().unwrap().revoke().unwrap();
    let mut coordinator = Box::pin(terminal.coordinate(probe.cleanup_with(charge), &deadlines));

    assert_eq!(
        poll_once(coordinator.as_mut()),
        Poll::Ready(BridgeCleanupOutcome::Quarantined)
    );
    assert_eq!(bytes.used(), 8);
    probe.release();
    assert_eq!(poll_cleanup_driver(&cleanup), Poll::Ready(Ok(true)));
    assert_eq!(bytes.used(), 0);
}

#[test]
fn a_late_operation_retains_the_connection_after_owner_cleanup() {
    let owner = owner();
    let connection = owner.open_connection().unwrap();
    let admission = connection.admission();
    let active = admission
        .begin(Arc::from("late"))
        .unwrap()
        .activate(BridgeOperationKind::Query)
        .unwrap();
    let terminal = connection.revoke().unwrap();
    let deadlines = ManualDeadline::default();
    let probe = CleanupProbe::ready();
    let mut coordinator = Box::pin(terminal.coordinate(probe.cleanup(), &deadlines));
    assert_eq!(
        poll_once(coordinator.as_mut()),
        Poll::Ready(BridgeCleanupOutcome::Completed)
    );

    assert_eq!(owner.delivery().snapshot().endpoint_connections, 1);
    assert_eq!(
        owner.open_connection().err(),
        Some(BridgeResourceError::Capacity)
    );
    assert_eq!(
        admission.begin(Arc::from("new")).err(),
        Some(BridgeResourceError::StaleGeneration)
    );
    drop(active.revoke().unwrap());
    assert_eq!(owner.delivery().snapshot().endpoint_connections, 0);
    assert!(owner.open_connection().is_ok());
}

#[test]
fn connection_and_operation_timeouts_occupy_distinct_cleanup_slots() {
    let owner = owner();
    let cleanup = owner.cleanup_driver();
    let connection = owner.open_connection().unwrap();
    let active = connection
        .begin(Arc::from("operation"))
        .unwrap()
        .activate(BridgeOperationKind::Query)
        .unwrap();
    let connection_terminal = connection.revoke().unwrap();
    let operation_terminal = active.revoke().unwrap();
    let deadlines = ManualDeadline::default();
    deadlines.expire();
    let connection_probe = CleanupProbe::default();
    let operation_probe = CleanupProbe::default();
    let mut connection_cleanup =
        Box::pin(connection_terminal.coordinate(connection_probe.cleanup(), &deadlines));
    let mut operation_cleanup =
        Box::pin(operation_terminal.coordinate(operation_probe.cleanup(), &deadlines));
    assert_eq!(
        poll_once(connection_cleanup.as_mut()),
        Poll::Ready(BridgeCleanupOutcome::Quarantined)
    );
    assert_eq!(
        poll_once(operation_cleanup.as_mut()),
        Poll::Ready(BridgeCleanupOutcome::Quarantined)
    );
    assert_eq!(cleanup.pending(), Ok(2));
    assert_eq!(owner.delivery().snapshot().endpoint_connections, 1);

    connection_probe.release();
    operation_probe.release();
    assert_eq!(poll_cleanup_driver(&cleanup), Poll::Ready(Ok(true)));
    assert_eq!(owner.delivery().snapshot().endpoint_connections, 1);
    assert_eq!(poll_cleanup_driver(&cleanup), Poll::Ready(Ok(true)));
    assert_eq!(owner.delivery().snapshot().endpoint_connections, 0);
    assert_eq!(connection_probe.drops(), 1);
    assert_eq!(operation_probe.drops(), 1);
}

#[test]
fn dropping_admission_clones_never_releases_the_owner_permit() {
    let owner = owner();
    let connection = owner.open_connection().unwrap();
    let first = connection.admission();
    let second = first.clone();
    drop(first);
    drop(second);
    assert_eq!(owner.delivery().snapshot().endpoint_connections, 1);
    drop(connection);
    assert_eq!(owner.delivery().snapshot().endpoint_connections, 0);
}
