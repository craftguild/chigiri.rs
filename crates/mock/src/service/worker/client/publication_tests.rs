//! Drive publication capture through the real FIFO worker and request commit owner.

use std::time::{Duration, Instant};

use chigiri::core::{Object, Value};

use super::super::{Client, Worker};
use crate::cancellation::Cancellation;
use crate::ir::{FieldId, ScriptId};
use crate::luau::Contract;
use crate::publication::test_support::usage;
use crate::runtime::{Execution, ExecutionBudget};
use crate::store::Store;

const SDL: &str = r#"
type Query { ping: Int! }
type Mutation { create(id: ID!): Item! second: Int! fail: Int! bad: Int! }
type Subscription { changed(case: String): Item! event: Int! }
type Item { id: ID! name: String! }
"#;

pub(super) fn fixture() -> Worker {
    fixture_with_setup(
        "local dynamic:any=ctx assert(dynamic.pubsub.publish==nil) return ctx.pubsub.subscribe.Item({topic='setup'})",
    )
}

fn fixture_with_setup(setup_body: &str) -> Worker {
    let ir = crate::service::tests::ir(SDL);
    let oversized = "x".repeat(4096);
    let bad_publication = format!(
        "local p:any=ctx.pubsub.publish.Int local ok=pcall(function() p({{topic='{oversized}',payload=1}}) end) assert(not ok) return 1"
    );
    let mut scripts = crate::service::tests::candidates(
        &ir,
        &[
            (
                "Mutation.create",
                "local row=ctx.models.Item:create({id=args.id,name=args.id}) ctx.pubsub.publish.Item({topic='created:' .. args.id,payload=row}) return row",
            ),
            (
                "Mutation.second",
                "ctx.pubsub.publish.Int({topic='second',payload=2}) return 2",
            ),
            ("Mutation.fail", "error('private later-root failure')"),
            ("Mutation.bad", &bad_publication),
            (
                "Query.ping",
                "local dynamic:any=ctx assert(dynamic.pubsub==nil) return 1",
            ),
        ],
    );
    let subscription = field_from_ir(&ir, "Subscription", "changed");
    let script = ScriptId(
        ir.scripts()
            .iter()
            .position(|plan| plan.field == subscription)
            .unwrap(),
    );
    scripts[script.0].source =
        crate::luau::render_subscription_module(&ir, script, setup_body, None).unwrap();
    Worker::start(
        Contract::new(ir.clone()).unwrap(),
        scripts,
        Store::empty(ir),
    )
    .unwrap()
}

pub(super) fn field(client: &Client, parent: &str, name: &str) -> FieldId {
    field_from_ir(client.ir(), parent, name)
}

fn field_from_ir(ir: &crate::ir::MockIr, parent: &str, name: &str) -> FieldId {
    let graph = ir.graph();
    graph.types()[graph.type_id(parent).unwrap().0].fields[name]
}

pub(super) fn execution(client: &Client) -> Execution {
    let budget = ExecutionBudget::default();
    budget.set_clock(&client.clock);
    Execution {
        state: client.initial_state().snapshot(),
        request: None,
        deadline: Instant::now() + Duration::from_secs(5),
        cancel: Cancellation::default(),
        shutdown: client.shutdown.clone(),
        budget,
    }
}

pub(super) async fn create(client: &Client, execution: &Execution, id: &str) {
    let arguments = Object::from_iter([("id", Value::String(id.to_owned()))]);
    client
        .execute_with(
            field(client, "Mutation", "create"),
            arguments,
            None,
            None,
            execution.clone(),
        )
        .await
        .unwrap()
        .output
        .unwrap();
}

#[tokio::test]
async fn real_multi_root_request_commits_current_record_identity() {
    let _case = crate::vm_gate::test_case();
    let mut worker = fixture();
    let execution = execution(&worker.client);
    let client = worker
        .client
        .request_client(true, &execution)
        .await
        .unwrap();
    create(&client, &execution, "first").await;
    client
        .execute_with(
            field(&client, "Mutation", "second"),
            Object::new(),
            None,
            None,
            execution.clone(),
        )
        .await
        .unwrap()
        .output
        .unwrap();
    client.prepare_request(execution.deadline).await.unwrap();
    client.commit_request().unwrap();
    assert_eq!(
        worker.client.committed.lock().unwrap().snapshot().size().0,
        1
    );
    drop(client);
    worker.stop(execution.deadline).await.unwrap();
}

#[tokio::test]
async fn later_root_failure_and_cancellation_discard_the_complete_request_capture() {
    let _case = crate::vm_gate::test_case();
    for cancel in [false, true] {
        let mut worker = fixture();
        let execution = execution(&worker.client);
        let client = worker
            .client
            .request_client(true, &execution)
            .await
            .unwrap();
        create(&client, &execution, "discarded").await;
        if cancel {
            execution.cancel.cancel();
        } else {
            assert!(
                client
                    .execute_with(
                        field(&client, "Mutation", "fail"),
                        Object::new(),
                        None,
                        None,
                        execution.clone(),
                    )
                    .await
                    .unwrap()
                    .output
                    .is_err()
            );
        }
        let prepared = client.prepare_request(execution.deadline).await;
        if cancel {
            let failure = prepared.expect_err("cancelled preparation must not run");
            assert!(failure.cancelled);
            assert_eq!(
                execution.budget.failure(),
                Some(crate::limits::request::BudgetFailure::Cancelled)
            );
        } else {
            assert!(prepared.is_err());
        }
        assert!(client.commit_request().is_err());
        assert_eq!(usage(&worker.client.committed.lock().unwrap()), (0, 0));
        assert_eq!(
            worker.client.committed.lock().unwrap().snapshot().size().0,
            0
        );
        drop(client);
        worker
            .stop(Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn real_query_request_never_receives_the_mutation_publication_capability() {
    let _case = crate::vm_gate::test_case();
    let mut worker = fixture();
    let output = worker
        .client
        .execute(field(&worker.client, "Query", "ping"), Object::new(), None)
        .await
        .unwrap();
    assert!(matches!(
        output,
        crate::output::OutputValue::Scalar(Value::Int(1))
    ));
    worker
        .stop(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
}

#[tokio::test]
async fn real_subscription_setup_vm_has_only_the_setup_binding_capability() {
    let _case = crate::vm_gate::test_case();
    let mut worker = fixture();
    let execution = execution(&worker.client);
    let binding = worker
        .client
        .subscription_setup(
            field(&worker.client, "Subscription", "changed"),
            Object::new(),
            None,
            execution.clone(),
        )
        .await
        .unwrap();
    assert_eq!(binding.topic().unwrap(), "setup");
    worker.stop(execution.deadline).await.unwrap();
}

#[tokio::test]
async fn caught_publication_failure_is_sticky_and_the_same_worker_recovers() {
    let _case = crate::vm_gate::test_case();
    let mut worker = fixture();
    assert!(
        worker
            .client
            .execute(
                field(&worker.client, "Mutation", "bad"),
                Object::new(),
                None,
            )
            .await
            .is_err()
    );
    let output = worker
        .client
        .execute(field(&worker.client, "Query", "ping"), Object::new(), None)
        .await
        .unwrap();
    assert!(matches!(
        output,
        crate::output::OutputValue::Scalar(Value::Int(1))
    ));
    worker
        .stop(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
}

#[tokio::test]
async fn setup_bounds_keys_topic_and_overflow_then_same_worker_recovers() {
    let _case = crate::vm_gate::test_case();
    let key_over = "k".repeat(9);
    let key_large = "k".repeat(4096);
    let topic_exact = "t".repeat(256);
    let topic_over = "t".repeat(257);
    let topic_large = "t".repeat(4096);
    let unicode_exact = "é".repeat(128);
    let unicode_over = "é".repeat(129);
    let overflow_large = "b".repeat(4096);
    let body = format!(
        r#"local d:any=ctx.pubsub.subscribe
local bad:any=nil
if args.case=='key_over' then bad={{['{key_over}']='x'}} end
if args.case=='key_large' then bad={{['{key_large}']='x'}} end
if args.case=='topic_over' then bad={{topic='{topic_over}'}} end
if args.case=='topic_large' then bad={{topic='{topic_large}'}} end
if args.case=='unicode_over' then bad={{topic='{unicode_over}'}} end
if args.case=='topic_control' then bad={{topic='bad\n'}} end
if args.case=='overflow_over' then bad={{topic='x',overflow='backpressurex'}} end
if args.case=='overflow_large' then bad={{topic='x',overflow='{overflow_large}'}} end
if bad~=nil then
    local ok=pcall(function() d.Item(bad) end)
    assert(not ok)
    return d.Item({{topic='unreachable'}})
end
if args.case=='topic_exact' then return d.Item({{topic='{topic_exact}'}}) end
if args.case=='unicode_exact' then return d.Item({{topic='{unicode_exact}'}}) end
if args.case=='overflow_exact' then return d.Item({{topic='x',overflow='backpressure'}}) end
return d.Item({{topic='setup'}})"#
    );
    let mut worker = fixture_with_setup(&body);
    let setup = |case: Option<&str>| {
        let arguments = Object::from_iter([(
            "case",
            case.map_or(Value::Null, |case| Value::String(case.to_owned())),
        )]);
        let execution = execution(&worker.client);
        worker.client.subscription_setup(
            field(&worker.client, "Subscription", "changed"),
            arguments,
            None,
            execution,
        )
    };
    for case in [
        "key_over",
        "key_large",
        "topic_over",
        "topic_large",
        "unicode_over",
        "topic_control",
        "overflow_over",
        "overflow_large",
    ] {
        assert!(setup(Some(case)).await.is_err(), "{case}");
        assert_eq!(setup(None).await.unwrap().topic().unwrap(), "setup");
    }
    assert_eq!(
        setup(Some("topic_exact"))
            .await
            .unwrap()
            .topic()
            .unwrap()
            .len(),
        256
    );
    assert_eq!(
        setup(Some("unicode_exact"))
            .await
            .unwrap()
            .topic()
            .unwrap()
            .len(),
        256
    );
    assert_eq!(
        setup(Some("overflow_exact")).await.unwrap().overflow(),
        chigiri::SubscriptionOverflow::Backpressure
    );
    worker
        .stop(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
}
