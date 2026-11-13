//! Exercise SDL-type selectors through actual setup, mutation commit and delivery.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

use chigiri::core::{Object, Value};
use futures_core::Stream;

use super::super::{Client, Worker};
use crate::cancellation::Cancellation;
use crate::ir::{FieldId, ScriptId};
use crate::luau::{Contract, ScriptCandidate, render_subscription_module};
use crate::output::{ObjectData, OutputValue};
use crate::publication::test_support::{register_source, shares_capture};
use crate::runtime::{Execution, ExecutionBudget};
use crate::store::Store;
use crate::value::ValueBudget;

const SDL: &str = r#"
type Query { a: Item }
type Mutation { emit(shape: Int!): Int! }
type Item { id: ID! title: String! }
type Other { id: ID! title: String! }
type Subscription { b: Item! c: Item! nullable: Item list: [Item!]! limited: [Item!]! @constraint(minItems: 2) other: Other! }
"#;

const EMIT: &str = r#"
local row = ctx.models.Item:create({id='one',title='captured'})
if args.shape == 0 then
    ctx.pubsub.publish.Item({topic='same',payload=row})
elseif args.shape == 1 then
    ctx.pubsub.publish.Item({topic='same',payload=ctx.null})
elseif args.shape == 2 then
    ctx.pubsub.publish.Item({topic='same',payload={row}})
else
    local publish:any = ctx.pubsub.publish.Item
    assert(not pcall(function() publish({topic='same',payload='invalid'}) end))
end
ctx.models.Item:update({where={field='id',op='eq',value='one'}},{title='current'})
return 1
"#;

fn fixture() -> Worker {
    let ir = crate::service::tests::ir(SDL);
    let mut scripts = crate::service::tests::candidates(&ir, &[("Mutation.emit", EMIT)]);
    for (index, plan) in ir.scripts().iter().enumerate() {
        if plan.role.is_subscription_root() {
            let selector = crate::events::payload::field_type_name(&ir, plan.field);
            scripts[index].source = render_subscription_module(
                &ir,
                ScriptId(index),
                &format!(
                    "local d:any=ctx assert(d.pubsub.subscribe.b==nil) assert(d.generator.sequence.b==nil) return ctx.pubsub.subscribe.{selector}({{topic='same'}})"
                ),
                None,
            )
            .unwrap();
        }
    }
    Worker::start(
        Contract::new(ir.clone()).unwrap(),
        scripts,
        Store::empty(ir),
    )
    .unwrap()
}

fn field(client: &Client, parent: &str, name: &str) -> FieldId {
    let graph = client.ir().graph();
    graph.types()[graph.type_id(parent).unwrap().0].fields[name]
}

fn execution(client: &Client) -> Execution {
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

struct Noop;
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}

fn poll<S: Stream + Unpin>(source: &mut S) -> Poll<Option<S::Item>> {
    Pin::new(source).poll_next(&mut Context::from_waker(&Waker::from(Arc::new(Noop))))
}

fn title(client: &Client, output: &OutputValue) {
    let OutputValue::Object(object) = output else {
        panic!("record required")
    };
    let ObjectData::CapturedRecord(record) = object.data() else {
        panic!("captured record provenance required")
    };
    let slot = client.ir().models()[record.model().0].slots["title"];
    assert_eq!(record.get(slot), Some(&Value::from("captured")));
}

#[tokio::test]
async fn item_type_broadcast_filters_complete_shapes_and_keeps_one_capture() {
    let _case = crate::vm_gate::test_case();
    for shape in 0..3 {
        let mut worker = fixture();
        let execution = execution(&worker.client);
        let fields: Vec<_> = ["b", "c", "nullable", "list", "limited", "other"]
            .map(|name| field(&worker.client, "Subscription", name))
            .into();
        let mut sources = Vec::new();
        for field in &fields {
            // Run the real setup VM before installing the selected destination.
            let binding = worker
                .client
                .subscription_setup(*field, Object::new(), None, execution.clone())
                .await
                .unwrap();
            assert_eq!(binding.topic().unwrap(), "same");
            sources.push(
                register_source(
                    &mut worker.client.committed.lock().unwrap(),
                    worker.client.ir(),
                    *field,
                    "same",
                )
                .into_parts()
                .0,
            );
        }
        for source in &mut sources {
            assert!(poll(source).is_pending());
        }
        let request = worker
            .client
            .request_client(true, &execution)
            .await
            .unwrap();
        request
            .execute_with(
                field(&request, "Mutation", "emit"),
                Object::from_iter([("shape", Value::Int(shape))]),
                None,
                None,
                execution.clone(),
            )
            .await
            .unwrap()
            .output
            .unwrap();
        request.prepare_request(execution.deadline).await.unwrap();
        request.commit_request().unwrap();
        drop(request);
        let selected: &[usize] = match shape {
            0 => &[0, 1, 2],
            1 => &[2],
            _ => &[3],
        };
        let mut events = Vec::new();
        for index in selected {
            let source = &mut sources[*index];
            let event = tokio::time::timeout(
                Duration::from_secs(2),
                std::future::poll_fn(|cx| Pin::new(&mut *source).poll_next(cx)),
            )
            .await
            .unwrap()
            .unwrap()
            .unwrap();
            let snapshot = worker.client.committed.lock().unwrap().snapshot();
            let output = event
                .project(snapshot, fields[*index], &mut ValueBudget::default())
                .unwrap();
            match (shape, output) {
                (0, output) => title(&worker.client, &output),
                (1, OutputValue::Null) => {}
                (2, OutputValue::List(values)) => {
                    assert_eq!(values.len(), 1);
                    title(&worker.client, &values[0]);
                }
                _ => panic!("destination shape changed"),
            }
            events.push(event);
        }
        for event in &events {
            assert!(shares_capture(&events[0], event));
        }
        // No duplicate delivery, no delivery or terminal loss to another shape/type.
        for source in &mut sources {
            assert!(poll(source).is_pending());
        }
        drop(events);
        drop(sources);
        worker.stop(execution.deadline).await.unwrap();
    }
}

#[tokio::test]
async fn no_matching_item_shape_poisoning_survives_a_catch_and_rolls_back() {
    let _case = crate::vm_gate::test_case();
    let mut worker = fixture();
    let execution = execution(&worker.client);
    let request = worker
        .client
        .request_client(true, &execution)
        .await
        .unwrap();
    let result = request
        .execute_with(
            field(&request, "Mutation", "emit"),
            Object::from_iter([("shape", Value::Int(3))]),
            None,
            None,
            execution.clone(),
        )
        .await
        .unwrap();
    assert!(result.output.is_err());
    assert!(request.prepare_request(execution.deadline).await.is_err());
    assert!(request.commit_request().is_err());
    assert_eq!(
        worker.client.committed.lock().unwrap().snapshot().size().0,
        0
    );
    drop(request);
    worker.stop(execution.deadline).await.unwrap();
}

#[test]
fn query_shared_item_uses_type_selectors_and_rejects_field_aliases() {
    let _case = crate::vm_gate::test_case();
    let contract = Contract::new(crate::service::tests::ir(SDL)).unwrap();
    let field =
        contract.ir().graph().types()[contract.ir().graph().subscription().unwrap().0].fields["b"];
    let id = ScriptId(
        contract
            .ir()
            .scripts()
            .iter()
            .position(|p| p.field == field)
            .unwrap(),
    );
    for (body, valid) in [
        ("return ctx.pubsub.subscribe.Item({topic='same'})", true),
        (
            "return ctx.generator.sequence.Item({values=ctx.models.Item:find({}),interval_ms=1})",
            true,
        ),
        ("return ctx.pubsub.subscribe.b({topic='same'})", false),
        (
            "return ctx.generator.sequence.b({values={},interval_ms=1})",
            false,
        ),
    ] {
        let source = render_subscription_module(contract.ir(), id, body, None).unwrap();
        assert_eq!(
            contract
                .script(ScriptCandidate { script: id, source })
                .is_ok(),
            valid,
            "{body}"
        );
    }
    let mutation = ScriptId(
        contract
            .ir()
            .scripts()
            .iter()
            .position(|p| p.role.is_mutation_root())
            .unwrap(),
    );
    let source = crate::luau::render_module(
        contract.ir(),
        mutation,
        "ctx.pubsub.publish.b({topic='same',payload=ctx.null}) return 1",
    )
    .unwrap();
    assert!(
        contract
            .script(ScriptCandidate {
                script: mutation,
                source
            })
            .is_err()
    );
}
