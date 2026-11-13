//! Test candidate ownership, rollback, and base-generation comparison at the store swap point.

use super::*;
use crate::store::tests::{BLOG, ir, model, object};
use crate::store::{CallScope, ModelId};

fn create(candidate: &mut RequestCandidate, model: ModelId, id: &str) {
    let mut root = candidate
        .store_mut()
        .begin(CallScope::MutationRoot)
        .unwrap();
    root.create(model, &object(&[("id", id.into()), ("name", id.into())]))
        .unwrap();
    candidate
        .store_mut()
        .commit(root.prepare().unwrap())
        .unwrap();
}

#[test]
fn multiple_root_changes_stay_candidate_local_and_commit_once_from_the_base_generation() {
    let ir = ir(BLOG);
    let user = model(&ir, "User");
    let mut committed = Store::empty(ir);
    let before = committed.snapshot();
    let mut candidate = committed.begin_request(ValueBudget::default());
    create(&mut candidate, user, "first");
    let first = candidate.snapshot();
    assert!(first.record(user, "first").is_some());
    create(&mut candidate, user, "second");
    assert!(candidate.snapshot().record(user, "first").is_some());
    assert!(candidate.snapshot().record(user, "second").is_some());
    assert!(first.record(user, "second").is_none());
    assert!(committed.snapshot().same_version(&before));
    let retired = committed
        .commit_request(&candidate.prepare().unwrap())
        .unwrap();
    assert!(retired.same_version(&before));
    assert_eq!(committed.snapshot().size().0, 2);
    assert_eq!(before.size().0, 0);
}

#[test]
fn dropping_a_failed_candidate_preserves_the_committed_generation() {
    let ir = ir(BLOG);
    let user = model(&ir, "User");
    let committed = Store::empty(ir);
    let before = committed.snapshot();
    let mut candidate = committed.begin_request(ValueBudget::default());
    create(&mut candidate, user, "first");
    let mut root = candidate
        .store_mut()
        .begin(CallScope::MutationRoot)
        .unwrap();
    assert!(
        root.create(
            user,
            &object(&[("id", "first".into()), ("name", "duplicate".into())])
        )
        .is_err()
    );
    assert!(root.prepare().is_err());
    drop(candidate);
    // State identity covers record identity, ordinal indexes, and capacity accounting.
    assert!(committed.snapshot().same_version(&before));
}

#[test]
fn a_candidate_is_rejected_after_another_request_commits_first() {
    let ir = ir(BLOG);
    let user = model(&ir, "User");
    let mut committed = Store::empty(ir);
    let mut first = committed.begin_request(ValueBudget::default());
    let mut stale = committed.begin_request(ValueBudget::default());
    create(&mut first, user, "winner");
    create(&mut stale, user, "stale");
    committed.commit_request(&first.prepare().unwrap()).unwrap();
    let winner = committed.snapshot();
    assert!(committed.commit_request(&stale.prepare().unwrap()).is_err());
    assert!(committed.snapshot().same_version(&winner));
}

#[test]
fn a_failed_shared_ledger_prevents_commit() {
    let mut committed = Store::empty(ir(BLOG));
    let before = committed.snapshot();
    let mut work = crate::value::work::Work::default();
    let candidate = committed.begin_request(ValueBudget::with_work(work.clone()));
    let prepared = candidate.prepare().unwrap();
    assert!(work.charge(usize::MAX).is_err());
    assert!(committed.commit_request(&prepared).is_err());
    assert!(committed.snapshot().same_version(&before));
}

#[test]
fn cancellation_fence_makes_commit_and_result_publication_atomic() {
    use crate::cancellation::Cancellation;
    use std::time::{Duration, Instant};

    for cancel_first in [true, false] {
        let ir = ir(BLOG);
        let user = model(&ir, "User");
        let mut committed = Store::empty(ir);
        let before = committed.snapshot();
        let mut candidate = committed.begin_request(ValueBudget::default());
        create(&mut candidate, user, "first");
        let prepared = candidate.prepare().unwrap();
        let cancel = Cancellation::default();
        let shutdown = Cancellation::default();
        if cancel_first {
            cancel.cancel();
        }
        let mut terminal_committed = false;
        let retired = cancel.commit(&shutdown, Instant::now() + Duration::from_secs(2), || {
            let retired = committed.commit_request(&prepared).unwrap();
            terminal_committed = true;
            retired
        });
        cancel.cancel();
        assert_eq!(terminal_committed, !cancel_first);
        assert_eq!(retired.is_some(), !cancel_first);
        assert_eq!(committed.snapshot().same_version(&before), cancel_first);
        assert_eq!(
            committed.snapshot().record(user, "first").is_some(),
            !cancel_first
        );
        // Dropping retired snapshots cannot roll back state that was already committed.
        drop((retired, prepared));
        assert_eq!(committed.snapshot().size().0, usize::from(!cancel_first));
    }
}
