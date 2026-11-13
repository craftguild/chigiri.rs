//! Deliver one terminal event while preserving internal failures discovered during teardown.

use super::*;
use crate::limits::request::{BudgetFailure, LimitKind, LimitNotice};
use crate::service::hooks::{completion::Failure, session::Session};

#[tokio::test]
async fn dropping_an_unreached_future_records_it_without_marking_the_slot_reached() {
    use crate::service::hooks::progress::After;
    use std::future::Future;
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        "type Query{value:Int!}",
        "{}",
        &[("Query.value", "return 1")],
        &[(HookSlot::FieldAfter, "return nil")],
    );
    let trace = crate::service::hooks::http::Trace::new(None, Arc::new(|_| {}));
    let client = &fixture.worker.client;
    drop(After::new(&trace, client, HookSlot::RequestAfter));
    let mut reached = After::new(&trace, client, HookSlot::FieldAfter);
    reached.reached();
    drop(reached);
    assert!(trace.unreached_after_hooks().is_empty());
    let mut pending = Box::pin(async {
        let _after = After::new(&trace, client, HookSlot::FieldAfter);
        std::future::pending::<()>().await;
    });
    std::future::poll_fn(|cx| {
        assert!(pending.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    assert!(trace.unreached_after_hooks().is_empty());
    drop(pending);
    for _ in 0..32 {
        drop(After::new(&trace, client, HookSlot::FieldAfter));
    }
    assert_eq!(trace.unreached_after_hooks(), vec![HookSlot::FieldAfter]);
    fixture.stop().await;
}

#[tokio::test]
async fn last_job_after_session_drop_sets_terminal_cause_once() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        "type Query{value:Int! @chigiri_mock_resolver}",
        "{}",
        &[("Query.value", "return 1")],
        &[],
    );
    for (late, expected) in [(true, "execution_failed"), (false, "rejected")] {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let scope = HttpScope::new("127.0.0.1:49123".parse().unwrap())
            .with_terminal(move |event| sink.lock().unwrap().push(event));
        let (job, budget) = scope
            .run(async {
                let session = Session::with_http(&fixture.worker.client, true);
                assert!(session.start(&fixture.worker.client, false).await);
                let job = session.client.get().unwrap().pending_job();
                let budget = session.trace.execution_budget.clone();
                session.fail(Failure::Rejected(
                    200,
                    chigiri::GraphQlError::new("rejected"),
                ));
                drop(session);
                (job, budget)
            })
            .await;
        drop(scope);
        assert!(events.lock().unwrap().is_empty());
        if late {
            budget.0.clone().fail_independent();
        } else {
            budget.0.clone().fail_with(BudgetFailure::Cancelled);
        }
        drop(job);
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].cause, expected);
        assert_eq!(events[0].transaction, "rolled_back");
        assert_eq!(events[0].status, None);
    }
    fixture.stop().await;
}

#[tokio::test]
async fn rejection_waits_for_unfinished_jobs_and_prioritizes_internal_failure() {
    use std::future::Future;
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        "type Query{value:Int! @chigiri_mock_resolver}",
        "{}",
        &[("Query.value", "return 1")],
        &[],
    );
    let session = Session::with_http(&fixture.worker.client, false);
    assert!(session.start(&fixture.worker.client, false).await);
    // Keep one worker job pending so finalization reevaluates failure precedence after rejection.
    let job = session.client.get().unwrap().pending_job();
    session.fail(Failure::Rejected(
        200,
        chigiri::GraphQlError::new("rejected"),
    ));
    let mut response =
        Box::pin(session.finish(chigiri::GraphQlResponse::executed(chigiri::Value::Null)));
    std::future::poll_fn(|cx| {
        assert!(response.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    session.trace.execution_budget.0.clone().fail_independent();
    drop(job);
    let response = response.await;
    assert!(matches!(
        *session.failure.lock().unwrap(),
        Some(Failure::Internal)
    ));
    assert_eq!(response.errors.len(), 1);
    assert_eq!(response.errors[0].message, "Mock execution failed");
    drop(session);
    fixture.stop().await;
}

#[tokio::test]
async fn terminal_drop_prioritizes_independent_ledger_failure_over_rejection() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        "type Query{value:Int! @chigiri_mock_resolver}",
        "{}",
        &[("Query.value", "return 1")],
        &[],
    );
    for (failure, cause) in [
        (BudgetFailure::Execution, "execution_failed"),
        (BudgetFailure::Deadline, "timeout"),
        (
            BudgetFailure::Hard(LimitNotice {
                kind: LimitKind::Work,
                observed: 1_000_001,
            }),
            "hard_limit",
        ),
    ] {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let scope = HttpScope::new("127.0.0.1:49123".parse().unwrap())
            .with_terminal(move |event| sink.lock().unwrap().push(event));
        scope
            .run(async {
                let session = Session::with_http(&fixture.worker.client, true);
                assert!(session.start(&fixture.worker.client, false).await);
                session.fail(Failure::Rejected(
                    200,
                    chigiri::GraphQlError::new("rejected"),
                ));
                session.trace.execution_budget.0.clone().fail_with(failure);
                drop(session);
            })
            .await;
        drop(scope);
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].cause, cause);
        assert_eq!(events[0].transaction, "rolled_back");
        assert_eq!(events[0].status, None);
        assert_eq!(events[0].phase, "drop");
    }
    fixture.stop().await;
}
