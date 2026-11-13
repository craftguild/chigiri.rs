//! Prove request-wide rollback with the real FIFO worker and reserved capture owner.

use super::tests::{add, execution, worker};
use super::*;
use crate::ir::FieldId;
use crate::output::OutputValue;
use crate::publication::test_support::{take_topics, usage};
use chigiri::core::Object;

pub(super) fn publish(client: &Client, topic: &str) {
    let request = client.request.as_ref().unwrap();
    let field = subscription(client);
    request
        .publications()
        .unwrap()
        .push(
            client.ir(),
            field,
            topic,
            &OutputValue::Scalar(chigiri::Value::from(topic)),
        )
        .unwrap();
}

pub(super) fn subscription(client: &Client) -> FieldId {
    let root = client.ir().graph().subscription().unwrap();
    client.ir().graph().types()[root.0].fields["event"]
}

pub(super) fn fixture() -> crate::service::Worker {
    let ir = crate::service::tests::ir(
        "type Query { users: [User!]! } type Mutation { add(id: ID!): User! fail: Int! } type Subscription { event: String! } type User { id: ID! name: String! }",
    );
    let mut scripts = crate::service::tests::candidates(
        &ir,
        &[
            ("Query.users", "return ctx.models.User:find({})"),
            (
                "Mutation.add",
                "return ctx.models.User:create({id=args.id,name=args.id})",
            ),
            ("Mutation.fail", "error('later-root-failure')"),
        ],
    );
    let subscription = subscription_field(&ir);
    let script = crate::ir::ScriptId(
        ir.scripts()
            .iter()
            .position(|plan| plan.field == subscription)
            .unwrap(),
    );
    scripts[script.0].source = crate::luau::render_subscription_module(
        &ir,
        script,
        "return ctx.pubsub.subscribe.String({topic='capacity'})",
        None,
    )
    .unwrap();
    crate::service::Worker::start(
        crate::luau::Contract::new(ir.clone()).unwrap(),
        scripts,
        Store::empty(ir),
    )
    .unwrap()
}

fn subscription_field(ir: &crate::ir::MockIr) -> FieldId {
    let root = ir.graph().subscription().unwrap();
    ir.graph().types()[root.0].fields["event"]
}

#[tokio::test]
async fn all_roots_share_one_precommit_capture_and_commit_once() {
    let _case = crate::vm_gate::test_case();
    let mut worker = fixture();
    let execution = execution(&worker.client);
    let client = worker
        .client
        .request_client(true, &execution)
        .await
        .unwrap();
    add(&client, &execution, "first").await;
    publish(&client, "first");
    add(&client, &execution, "second").await;
    publish(&client, "second");
    assert_eq!(usage(&worker.client.committed.lock().unwrap()), (0, 0));
    client.prepare_request(execution.deadline).await.unwrap();
    assert_eq!(usage(&worker.client.committed.lock().unwrap()).0, 1);
    assert!(take_topics(&worker.client.committed.lock().unwrap()).is_empty());
    client.commit_request().unwrap();
    client.commit_request().unwrap();
    assert_eq!(
        worker.client.committed.lock().unwrap().snapshot().size().0,
        2
    );
    drop(client);
    worker.stop(execution.deadline).await.unwrap();
}

#[tokio::test]
async fn later_root_failure_discards_earlier_capture_and_store_changes() {
    let _case = crate::vm_gate::test_case();
    let mut worker = fixture();
    let execution = execution(&worker.client);
    let client = worker
        .client
        .request_client(true, &execution)
        .await
        .unwrap();
    add(&client, &execution, "discarded").await;
    publish(&client, "must-not-publish");
    assert!(
        client
            .execute_with(
                super::tests::field(&client, "fail"),
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
    assert!(client.commit_request().is_err());
    assert!(take_topics(&worker.client.committed.lock().unwrap()).is_empty());
    assert_eq!(
        worker.client.committed.lock().unwrap().snapshot().size().0,
        0
    );
    drop(client);
    worker.stop(execution.deadline).await.unwrap();
}

#[tokio::test]
async fn cancellation_after_reservation_releases_without_publishing_or_committing() {
    let _case = crate::vm_gate::test_case();
    let mut worker = fixture();
    let execution = execution(&worker.client);
    let client = worker
        .client
        .request_client(true, &execution)
        .await
        .unwrap();
    add(&client, &execution, "discarded").await;
    publish(&client, "must-not-publish");
    client.prepare_request(execution.deadline).await.unwrap();
    assert_eq!(usage(&worker.client.committed.lock().unwrap()).0, 1);
    execution.cancel.cancel();
    assert!(client.commit_request().is_err());
    assert_eq!(usage(&worker.client.committed.lock().unwrap()), (0, 0));
    assert!(take_topics(&worker.client.committed.lock().unwrap()).is_empty());
    assert_eq!(
        worker.client.committed.lock().unwrap().snapshot().size().0,
        0
    );
    drop(client);
    worker
        .stop(Instant::now() + std::time::Duration::from_secs(5))
        .await
        .unwrap();
}

#[tokio::test]
async fn dispatcher_loss_after_prepare_cannot_turn_a_store_win_into_rollback() {
    let _case = crate::vm_gate::test_case();
    let mut worker = fixture();
    let execution = execution(&worker.client);
    let client = worker
        .client
        .request_client(true, &execution)
        .await
        .unwrap();
    add(&client, &execution, "committed").await;
    publish(&client, "lost-after-win");
    client.prepare_request(execution.deadline).await.unwrap();
    let staging = worker.client.committed.lock().unwrap().staging();
    staging.close().unwrap();
    client.commit_request().unwrap();
    execution.cancel.cancel();
    assert!(client.request_committed());
    assert_eq!(
        worker.client.committed.lock().unwrap().snapshot().size().0,
        1
    );
    assert!(take_topics(&worker.client.committed.lock().unwrap()).is_empty());
    assert_eq!(usage(&worker.client.committed.lock().unwrap()), (0, 0));
    drop(client);
    worker
        .stop(Instant::now() + std::time::Duration::from_secs(5))
        .await
        .unwrap();
}

#[tokio::test]
async fn a_sticky_invalid_publication_fails_prepare_and_keeps_the_store_unchanged() {
    let _case = crate::vm_gate::test_case();
    let mut worker = fixture();
    let execution = execution(&worker.client);
    let client = worker
        .client
        .request_client(true, &execution)
        .await
        .unwrap();
    add(&client, &execution, "discarded").await;
    publish(&client, "valid-before-failure");
    let buffer = client.request.as_ref().unwrap().publications().unwrap();
    assert!(
        buffer
            .push(
                client.ir(),
                subscription(&client),
                "bad\n",
                &OutputValue::Null
            )
            .is_err()
    );
    assert!(client.prepare_request(execution.deadline).await.is_err());
    assert!(execution.budget.failure().is_some());
    assert!(client.commit_request().is_err());
    assert_eq!(
        worker.client.committed.lock().unwrap().snapshot().size().0,
        0
    );
    assert!(take_topics(&worker.client.committed.lock().unwrap()).is_empty());
    drop(client);
    worker
        .stop(Instant::now() + std::time::Duration::from_secs(5))
        .await
        .unwrap();
}

#[tokio::test]
async fn query_requests_never_acquire_a_publication_buffer() {
    let _case = crate::vm_gate::test_case();
    let mut worker = worker();
    let execution = execution(&worker.client);
    let client = worker
        .client
        .request_client(false, &execution)
        .await
        .unwrap();
    assert!(client.request.as_ref().unwrap().publications().is_none());
    client.prepare_request(execution.deadline).await.unwrap();
    client.commit_request().unwrap();
    assert_eq!(usage(&worker.client.committed.lock().unwrap()), (0, 0));
    drop(client);
    worker.stop(execution.deadline).await.unwrap();
}
