//! Test mutation ownership, waiter limits, cancellation notification, and waiter-slot reuse.

use std::future::{Future, poll_fn};
use std::task::Poll;

use super::*;
use crate::runtime::Execution;
use std::time::Instant;

fn execution(client: &Client) -> Execution {
    Execution {
        state: client.initial_state().snapshot(),
        request: None,
        deadline: Instant::now() + crate::limits::OPERATION_TIME,
        cancel: Cancellation::default(),
        shutdown: client.shutdown.clone(),
        budget: Default::default(),
    }
}

#[tokio::test]
async fn mutation_waiter_deadline_releases_its_queue_permit_without_releasing_the_owner() {
    let ir = crate::service::tests::ir("type Query { value: Int }");
    let (client, _) = queue(ir);
    let initial = execution(&client);
    let owner = client.request_client(true, &initial).await.unwrap();
    let mut next = execution(&client);
    next.deadline = Instant::now() + std::time::Duration::from_millis(30);
    let mut waiting = Box::pin(client.request_client(true, &next));
    poll_fn(|cx| {
        assert!(waiting.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert_eq!(
        client.mutation_waiters.available_permits(),
        crate::limits::QUEUED_JOBS - 1
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(1), waiting)
            .await
            .unwrap()
            .is_err()
    );
    assert_eq!(
        next.budget.failure(),
        Some(crate::limits::request::BudgetFailure::Deadline)
    );
    assert_eq!(
        client.mutation_waiters.available_permits(),
        crate::limits::QUEUED_JOBS
    );
    assert!(client.mutation.clone().try_lock_owned().is_err());
    drop(owner);
    assert!(
        client
            .request_client(true, &execution(&client))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn mutation_waiter_capacity_is_bounded_and_reusable_after_cancellation() {
    let ir = crate::service::tests::ir("type Query { value: Int }");
    let (client, _) = queue(ir);
    let initial = execution(&client);
    let owner = client.request_client(true, &initial).await.unwrap();
    let executions: Vec<_> = (0..crate::limits::QUEUED_JOBS)
        .map(|_| execution(&client))
        .collect();
    let mut waiting: Vec<_> = executions
        .iter()
        .map(|execution| Box::pin(client.request_client(true, execution)))
        .collect();
    poll_fn(|cx| {
        for future in &mut waiting {
            assert!(future.as_mut().poll(cx).is_pending());
        }
        Poll::Ready(())
    })
    .await;
    assert_eq!(client.mutation_waiters.available_permits(), 0);
    let excess = execution(&client);
    assert!(client.request_client(true, &excess).await.is_err());
    executions[0].cancel.cancel();
    // Keep the owner held while one waiting acquisition observes cancellation.
    poll_fn(|cx| {
        assert!(matches!(waiting[0].as_mut().poll(cx), Poll::Ready(Err(_))));
        Poll::Ready(())
    })
    .await;
    assert_eq!(
        executions[0].budget.failure(),
        Some(crate::limits::request::BudgetFailure::Cancelled)
    );
    assert_eq!(client.mutation_waiters.available_permits(), 1);
    drop(waiting);
    assert_eq!(
        client.mutation_waiters.available_permits(),
        crate::limits::QUEUED_JOBS
    );
    drop(owner);
    let recovered = client
        .request_client(true, &execution(&client))
        .await
        .unwrap();
    assert_eq!(
        recovered.snapshot(initial.deadline).await.unwrap().size().0,
        0
    );
}

#[tokio::test]
async fn shutdown_ends_a_mutation_wait_before_the_owner_releases() {
    let ir = crate::service::tests::ir("type Query { value: Int }");
    let (client, _) = queue(ir);
    let initial = execution(&client);
    let _owner = client.request_client(true, &initial).await.unwrap();
    let next = execution(&client);
    let mut waiting = Box::pin(client.request_client(true, &next));
    poll_fn(|cx| {
        assert!(waiting.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    client.shutdown.cancel();
    poll_fn(|cx| {
        assert!(matches!(waiting.as_mut().poll(cx), Poll::Ready(Err(_))));
        Poll::Ready(())
    })
    .await;
    assert_eq!(
        client.mutation_waiters.available_permits(),
        crate::limits::QUEUED_JOBS
    );
}
