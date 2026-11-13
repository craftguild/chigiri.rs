//! Test accepted boundaries, pre-reservation denial, shared notifications, and deadlines without sleeping.

use super::*;
use crate::limits::request::KINDS;
use std::time::Duration;

#[test]
fn elapsed_soft_limit_emits_one_notice_without_overwriting_a_failure() {
    let start = Instant::now();
    let mut ledger = Ledger::default();
    ledger.started = Some(start);
    ledger.observe_elapsed_soft(start + Duration::from_secs(6), &Default::default());
    assert!(ledger.failure.is_none());
    let notice = ledger.pending[LimitKind::ElapsedMs as usize]
        .take()
        .unwrap();
    assert_eq!(notice.observed, 6000);
    ledger.observe_elapsed_soft(start + Duration::from_secs(7), &Default::default());
    assert!(ledger.pending.iter().all(Option::is_none));
    ledger.failure = Some(BudgetFailure::Execution);
    ledger.observe_elapsed_soft(start + Duration::from_secs(8), &Default::default());
    assert_eq!(ledger.failure, Some(BudgetFailure::Execution));
}

#[test]
fn soft_notification_runs_once_outside_the_ledger_lock() {
    let mut work = Work::default();
    let ledger = Arc::downgrade(&work.ledger);
    let notices = Arc::new(Mutex::new(Vec::new()));
    let received = notices.clone();
    work.with_warnings(move |notice| {
        assert!(ledger.upgrade().unwrap().try_lock().is_ok());
        received.lock().unwrap().push(notice);
    });
    let policy = policy(LimitKind::Work);
    work.charge(policy.soft - 1).unwrap();
    assert!(notices.lock().unwrap().is_empty());
    let mut sibling = work.clone();
    sibling.charge(1).unwrap();
    work.charge(policy.hard - policy.soft).unwrap();
    assert!(work.failure().is_none());
    assert_eq!(
        notices
            .lock()
            .unwrap()
            .iter()
            .map(|warning| warning.notice())
            .collect::<Vec<_>>()
            .as_slice(),
        &[LimitNotice {
            kind: LimitKind::Work,
            observed: policy.soft,
        }]
    );
    assert!(sibling.charge(1).is_err());
    assert_eq!(notices.lock().unwrap().len(), 1);
}

fn counts(kind: LimitKind, amount: usize) -> [usize; 5] {
    let mut counts = [0; 5];
    counts[kind as usize] = amount;
    counts
}

#[test]
fn all_six_resource_policies_keep_the_approved_values() {
    let expected = [
        (750_000, 1_000_000),
        (75_000, 100_000),
        (12_582_912, 16_777_216),
        (3072, 4096),
        (786_432, 1_048_576),
        (3750, 5000),
    ];
    for (kind, (soft, hard)) in KINDS.into_iter().zip(expected) {
        assert_eq!((policy(kind).soft, policy(kind).hard), (soft, hard));
    }
}

#[test]
fn count_resources_notify_at_soft_and_fail_above_hard_limits() {
    let now = Instant::now();
    for kind in KINDS.into_iter().take(5) {
        let limit = policy(kind);
        let mut work = Work::default();
        work.reserve(counts(kind, limit.soft - 1), now).unwrap();
        assert!(work.take_warnings().is_empty());
        let mut after = work.clone();
        after.reserve(counts(kind, 1), now).unwrap();
        assert_eq!(
            work.take_warnings(),
            vec![LimitNotice {
                kind,
                observed: limit.soft
            }]
        );
        assert!(work.failure().is_none());
        after
            .reserve(counts(kind, limit.hard - limit.soft), now)
            .unwrap();
        assert!(work.take_warnings().is_empty());
        assert!(after.reserve(counts(kind, 1), now).is_err());
        assert_eq!(
            work.failure(),
            Some(BudgetFailure::Hard(LimitNotice {
                kind,
                observed: limit.hard + 1
            }))
        );
        assert_eq!(work.ledger.lock().unwrap().used[kind as usize], limit.hard);
        assert!(work.check().is_err());
        assert!(work.fork_startup().is_err());
    }
}

#[test]
fn failed_soft_to_hard_reservation_adds_neither_usage_nor_notice() {
    let now = Instant::now();
    for kind in KINDS.into_iter().take(5) {
        let limit = policy(kind);
        let mut work = Work::default();
        work.reserve(counts(kind, limit.soft - 1), now).unwrap();
        assert!(
            work.reserve(counts(kind, limit.hard - limit.soft + 2), now)
                .is_err()
        );
        assert!(work.take_warnings().is_empty());
        assert_eq!(
            work.ledger.lock().unwrap().used[kind as usize],
            limit.soft - 1
        );
    }
}

#[test]
fn aggregate_reservation_is_atomic_when_any_resource_exceeds_its_limit() {
    let mut work = Work::default();
    let counts = [
        policy(LimitKind::Work).soft,
        policy(LimitKind::ValueNodes).hard + 1,
        0,
        0,
        0,
    ];
    assert!(work.reserve(counts, Instant::now()).is_err());
    assert_eq!(work.ledger.lock().unwrap().used, [0; 6]);
    assert!(work.take_warnings().is_empty());
    assert!(matches!(
        work.failure(),
        Some(BudgetFailure::Hard(LimitNotice {
            kind: LimitKind::ValueNodes,
            ..
        }))
    ));
}

#[test]
fn elapsed_time_notifies_at_soft_and_fails_at_hard_limit() {
    let started = Instant::now();
    let mut work = Work::default();
    work.start_request(started);
    work.reserve([0; 5], started + Duration::from_millis(3749))
        .unwrap();
    assert!(work.take_warnings().is_empty());
    work.reserve([0; 5], started + Duration::from_millis(3750))
        .unwrap();
    assert_eq!(
        work.take_warnings(),
        vec![LimitNotice {
            kind: LimitKind::ElapsedMs,
            observed: 3750
        }]
    );
    work.start_request(started + Duration::from_secs(4));
    work.reserve([0; 5], started + Duration::from_millis(4999))
        .unwrap();
    assert!(work.take_warnings().is_empty());
    assert!(
        work.reserve([0; 5], started + Duration::from_millis(5000))
            .is_err()
    );
    assert_eq!(work.failure(), Some(BudgetFailure::Deadline));
}

#[test]
fn local_vm_deadline_and_cleanup_cancellation_hard_not_overwriting_cause() {
    let now = Instant::now();
    let cancel = Cancellation::default();
    let shutdown = Cancellation::default();
    let mut work = Work::default();
    work.start_request(now);
    work.guard(&cancel, &shutdown, now + Duration::from_millis(500));
    assert!(
        work.reserve([0; 5], now + Duration::from_millis(500))
            .is_err()
    );
    assert_eq!(work.failure(), Some(BudgetFailure::Deadline));
    let mut work = Work::default();
    work.charge(usize::MAX).unwrap_err();
    let first = work.failure();
    cancel.cancel();
    work.guard(&cancel, &shutdown, now);
    assert!(work.charge(0).is_err());
    assert_eq!(work.failure(), first);
}

#[test]
fn value_budget_propagates_hard_node_and_byte_failures_to_work() {
    use crate::value::{MAX_BYTES, MAX_NODES, ValueBudget};
    let work = Work::default();
    let mut values = ValueBudget::with_work(work.clone());
    for _ in 0..MAX_NODES {
        values.container(1).unwrap();
    }
    assert!(values.container(1).is_err());
    assert_eq!(values.nodes(), MAX_NODES);
    assert_eq!(
        work.failure(),
        Some(BudgetFailure::Hard(LimitNotice {
            kind: LimitKind::ValueNodes,
            observed: MAX_NODES + 1,
        }))
    );
    let work = Work::default();
    let mut values = ValueBudget::with_work(work.clone());
    assert!(values.string_node(MAX_BYTES + 1, 1).is_err());
    assert_eq!(values.bytes(), 0);
    assert_eq!(
        work.failure(),
        Some(BudgetFailure::Hard(LimitNotice {
            kind: LimitKind::LogicalBytes,
            observed: MAX_BYTES + 1,
        }))
    );
}
