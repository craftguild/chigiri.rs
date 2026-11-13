//! Send multiple VM roots to one request candidate and verify prepare, closure, and final publication.

use super::*;
use crate::ir::FieldId;
use crate::luau::Contract;
use crate::service::Worker;
use chigiri::core::Object;

pub(super) fn worker() -> Worker {
    let ir = crate::service::tests::ir(
        "type Query { users: [User!]! } type Mutation { add(id: ID!): User! fail: Int! } type User { id: ID! name: String! }",
    );
    let scripts = crate::service::tests::candidates(
        &ir,
        &[
            ("Query.users", "return ctx.models.User:find({})"),
            (
                "Mutation.add",
                "return ctx.models.User:create({id=args.id,name=args.id})",
            ),
            ("Mutation.fail", "error('private-root-failure')"),
        ],
    );
    Worker::start(
        Contract::new(ir.clone()).unwrap(),
        scripts,
        Store::empty(ir),
    )
    .unwrap()
}

pub(super) fn field(client: &Client, name: &str) -> FieldId {
    let graph = client.ir().graph();
    graph.types()[graph.mutation().unwrap().0].fields[name]
}

pub(super) fn execution(client: &Client) -> Execution {
    Execution {
        state: client.initial_state().snapshot(),
        request: None,
        deadline: Instant::now() + crate::limits::OPERATION_TIME,
        cancel: Cancellation::default(),
        shutdown: client.shutdown.clone(),
        budget: Default::default(),
    }
}

pub(super) async fn add(client: &Client, execution: &Execution, id: &str) {
    let args = [("id", chigiri::Value::from(id))].into_iter().collect();
    client
        .execute_with(field(client, "add"), args, None, None, execution.clone())
        .await
        .unwrap()
        .output
        .unwrap();
}

#[tokio::test]
async fn candidate_roots_publish_only_after_prepare_and_commit() {
    let _test_case = crate::vm_gate::test_case();
    let mut worker = worker();
    let execution = execution(&worker.client);
    let client = worker
        .client
        .request_client(true, &execution)
        .await
        .unwrap();
    add(&client, &execution, "first").await;
    add(&client, &execution, "second").await;
    assert_eq!(
        client.snapshot(execution.deadline).await.unwrap().size().0,
        2
    );
    assert_eq!(
        worker
            .client
            .snapshot(execution.deadline)
            .await
            .unwrap()
            .size()
            .0,
        0
    );
    client.prepare_request(execution.deadline).await.unwrap();
    assert_eq!(
        worker
            .client
            .snapshot(execution.deadline)
            .await
            .unwrap()
            .size()
            .0,
        0
    );
    // After the receiver closes, the root job never enters the queue or changes the candidate.
    assert!(
        client
            .execute_with(
                field(&client, "fail"),
                Object::new(),
                None,
                None,
                execution.clone()
            )
            .await
            .is_err()
    );
    client.request.as_ref().unwrap().commit(None).unwrap();
    execution.cancel.cancel();
    assert_eq!(
        worker
            .client
            .snapshot(execution.deadline)
            .await
            .unwrap()
            .size()
            .0,
        2
    );
    client.request.as_ref().unwrap().commit(None).unwrap();
    assert_eq!(
        worker
            .client
            .snapshot(execution.deadline)
            .await
            .unwrap()
            .size()
            .0,
        2
    );
    drop(client);
    worker.stop(execution.deadline).await.unwrap();
}

#[tokio::test]
async fn a_later_vm_failure_rolls_back_prior_unpublished_roots_and_the_worker_recovers() {
    let _test_case = crate::vm_gate::test_case();
    let mut worker = worker();
    let execution = execution(&worker.client);
    let client = worker
        .client
        .request_client(true, &execution)
        .await
        .unwrap();
    add(&client, &execution, "discarded").await;
    assert!(
        client
            .execute_with(
                field(&client, "fail"),
                Object::new(),
                None,
                None,
                execution.clone()
            )
            .await
            .unwrap()
            .output
            .is_err()
    );
    assert!(client.prepare_request(execution.deadline).await.is_err());
    assert!(client.request.as_ref().unwrap().commit(None).is_err());
    drop(client);
    assert_eq!(
        worker
            .client
            .snapshot(execution.deadline)
            .await
            .unwrap()
            .size()
            .0,
        0
    );
    let args = [("id", chigiri::Value::from("recovered"))]
        .into_iter()
        .collect();
    worker
        .client
        .execute(field(&worker.client, "add"), args, None)
        .await
        .unwrap();
    assert_eq!(
        worker
            .client
            .snapshot(execution.deadline)
            .await
            .unwrap()
            .size()
            .0,
        1
    );
    worker.stop(execution.deadline).await.unwrap();
}

#[tokio::test]
async fn a_shared_ledger_failure_after_prepare_still_prevents_query_commit() {
    let _test_case = crate::vm_gate::test_case();
    let mut worker = worker();
    let execution = execution(&worker.client);
    let client = worker
        .client
        .request_client(false, &execution)
        .await
        .unwrap();
    client.prepare_request(execution.deadline).await.unwrap();
    let mut work = execution.budget.0.clone();
    assert!(work.charge(usize::MAX).is_err());
    assert!(client.request.as_ref().unwrap().commit(None).is_err());
    assert_eq!(
        worker
            .client
            .snapshot(execution.deadline)
            .await
            .unwrap()
            .size()
            .0,
        0
    );
    drop(client);
    worker.stop(execution.deadline).await.unwrap();
}

#[tokio::test]
async fn dropping_the_response_future_cancels_the_queued_mutation() {
    use std::future::Future;
    let ir = crate::service::tests::ir(
        "type Query { users: [User!]! } type Mutation { change: Int } type User { id: ID! name: String! }",
    );
    let (base, receiver) = super::super::queue(ir);
    let execution = execution(&base);
    let client = base.request_client(true, &execution).await.unwrap();
    let user = client
        .ir()
        .model_for(client.ir().graph().type_id("User").unwrap())
        .unwrap();
    client
        .request
        .as_ref()
        .unwrap()
        .execute(|store| {
            let mut root = store.begin(crate::store::CallScope::MutationRoot).unwrap();
            let row: Object = [
                ("id", chigiri::Value::from("private")),
                ("name", chigiri::Value::from("private")),
            ]
            .into_iter()
            .collect();
            root.create(user, &row).unwrap();
            store.commit(root.prepare().unwrap()).unwrap();
            Ok(())
        })
        .unwrap();
    assert_eq!(
        client.snapshot(execution.deadline).await.unwrap().size().0,
        1
    );
    let mut pending = Box::pin(client.execute_with(
        field(&client, "change"),
        Object::new(),
        None,
        None,
        execution.clone(),
    ));
    std::future::poll_fn(|cx| {
        assert!(pending.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    assert_eq!(receiver.len(), 1);
    drop(pending);
    assert!(receiver.try_recv().unwrap().cancel.cancelled());
    assert!(client.prepare_request(execution.deadline).await.is_err());
    assert!(client.request.as_ref().unwrap().commit(None).is_err());
    assert!(receiver.is_empty());
    assert_eq!(base.committed.lock().unwrap().snapshot().size().0, 0);
    drop(client);
    // Dropping the client cancels its worker without awaiting execution and returns admission to the next request.
    let next_execution = super::tests::execution(&base);
    let next = base.request_client(true, &next_execution).await.unwrap();
    assert_eq!(next.snapshot(execution.deadline).await.unwrap().size().0, 0);
}

#[path = "request_failure_tests.rs"]
mod failure;
