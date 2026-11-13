//! Verify worker-contract and native failures are shared before delivery and prevent candidate publication.

use super::*;

#[tokio::test]
async fn pre_delivery_failure_preserves_deadline_shutdown_and_execution_causes() {
    use crate::limits::request::BudgetFailure;
    for cause in [
        BudgetFailure::Deadline,
        BudgetFailure::Cancelled,
        BudgetFailure::Execution,
    ] {
        let ir = crate::service::tests::ir("type Query{value:Int! @chigiri_mock_resolver}");
        let (base, receiver) = super::super::super::queue(ir);
        let mut execution = execution(&base);
        let client = base.request_client(false, &execution).await.unwrap();
        let graph = client.ir().graph();
        let field = graph.types()[graph.query().0].fields["value"];
        match cause {
            BudgetFailure::Deadline => execution.deadline = Instant::now(),
            BudgetFailure::Cancelled => base.shutdown.cancel(),
            BudgetFailure::Execution => drop(receiver),
            _ => unreachable!(),
        }
        let error = client
            .execute_with(field, Object::new(), None, None, execution.clone())
            .await
            .err()
            .expect("the request must fail before delivery");
        assert_eq!(error.cancelled, cause == BudgetFailure::Cancelled);
        assert_eq!(execution.budget.failure(), Some(cause));
        assert!(execution.cancel.cancelled());
        assert!(client.request.as_ref().unwrap().commit(None).is_err());
    }
}

#[tokio::test]
async fn a_prior_request_failure_makes_follow_up_input_report_cleanup_cancellation() {
    use crate::limits::request::BudgetFailure;
    let ir = crate::service::tests::ir("type Query{value:Int! @chigiri_mock_resolver}");
    let (base, _receiver) = super::super::super::queue(ir);
    let execution = execution(&base);
    let client = base.request_client(false, &execution).await.unwrap();
    client
        .request
        .as_ref()
        .unwrap()
        .fail_with(BudgetFailure::Execution);
    let graph = client.ir().graph();
    let field = graph.types()[graph.query().0].fields["value"];
    let error = client
        .execute_with(field, Object::new(), None, None, execution.clone())
        .await
        .err()
        .expect("a failed request must not be reopened");
    assert!(error.cancelled);
    assert_eq!(execution.budget.failure(), Some(BudgetFailure::Execution));
}

#[tokio::test]
async fn delayed_follow_up_after_failure_does_not_replace_the_original_cause() {
    let ir = crate::service::tests::ir("type Query{value:Int! @chigiri_mock_resolver}");
    let (base, _receiver) = super::super::super::queue(ir);
    let mut execution = execution(&base);
    let client = base.request_client(false, &execution).await.unwrap();
    client.request.as_ref().unwrap().fail();
    execution.deadline = Instant::now();
    let graph = client.ir().graph();
    let field = graph.types()[graph.query().0].fields["value"];
    let error = client
        .execute_with(field, Object::new(), None, None, execution.clone())
        .await
        .err()
        .expect("reply sender must be available");
    assert!(error.cancelled);
    assert!(execution.budget.failure().is_none());
}

#[tokio::test]
async fn queued_failure_preserves_cause_before_cleanup_closes_the_channel() {
    use crate::limits::request::BudgetFailure;
    use std::future::Future;
    for (timeout, rejected) in [(false, false), (false, true), (true, false)] {
        let ir = crate::service::tests::ir("type Query{value:Int! @chigiri_mock_resolver}");
        let (base, receiver) = super::super::super::queue(ir);
        let mut execution = execution(&base);
        execution.deadline = Instant::now() + std::time::Duration::from_millis(100);
        let client = base.request_client(false, &execution).await.unwrap();
        let graph = client.ir().graph();
        let field = graph.types()[graph.query().0].fields["value"];
        let mut pending =
            Box::pin(client.execute_with(field, Object::new(), None, None, execution.clone()));
        std::future::poll_fn(|cx| {
            assert!(pending.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        let job = receiver.try_recv().unwrap();
        let token = job.cancel.clone();
        if rejected {
            client.request.as_ref().unwrap().fail();
        }
        let held = if timeout {
            Some(job)
        } else {
            drop(job);
            None
        };
        let error = pending
            .await
            .err()
            .expect("the failed request must not return a response");
        assert!(!error.cancelled);
        assert_eq!(
            execution.budget.failure(),
            Some(if timeout {
                BudgetFailure::Deadline
            } else {
                BudgetFailure::Execution
            })
        );
        assert!(token.cancelled());
        assert!(client.request.as_ref().unwrap().commit(None).is_err());
        drop(held);
    }
}

#[tokio::test]
async fn worker_contract_failures_return_results_and_mark_the_shared_ledger() {
    let _case = crate::vm_gate::test_case();
    let mut worker = worker();
    let graph = worker.client.ir().graph();
    let user = graph
        .types()
        .iter()
        .position(|ty| ty.name.as_ref() == "User")
        .unwrap();
    let nested = graph.types()[user].fields["name"];
    for field in [FieldId(usize::MAX), nested] {
        let execution = execution(&worker.client);
        let client = worker
            .client
            .request_client(false, &execution)
            .await
            .unwrap();
        let result = client
            .execute_with(field, Object::new(), None, None, execution.clone())
            .await
            .unwrap();
        assert!(result.output.is_err());
        assert_eq!(
            execution.budget.failure(),
            Some(crate::limits::request::BudgetFailure::Execution)
        );
        assert_eq!(
            worker.client.committed.lock().unwrap().snapshot().size().0,
            0
        );
    }
    let execution = execution(&worker.client);
    let client = worker
        .client
        .request_client(true, &execution)
        .await
        .unwrap();
    let args = [("id", chigiri::Value::from("private"))]
        .into_iter()
        .collect();
    let output = client
        .execute_with(field(&client, "add"), args, None, None, execution.clone())
        .await
        .unwrap()
        .output
        .unwrap();
    let crate::output::OutputValue::Object(parent) = output else {
        panic!("model value required")
    };
    let bad_args = [("unknown", chigiri::Value::from("private"))]
        .into_iter()
        .collect();
    let result = client
        .execute_with(nested, bad_args, Some(parent), None, execution.clone())
        .await
        .unwrap();
    assert!(result.output.is_err());
    assert_eq!(
        execution.budget.failure(),
        Some(crate::limits::request::BudgetFailure::Execution)
    );
    assert!(client.prepare_request(execution.deadline).await.is_err());
    assert_eq!(
        worker.client.committed.lock().unwrap().snapshot().size().0,
        0
    );
    drop(client);
    worker
        .stop(Instant::now() + crate::limits::OPERATION_TIME)
        .await
        .unwrap();
}
