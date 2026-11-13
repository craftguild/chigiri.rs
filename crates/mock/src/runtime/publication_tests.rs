//! Exercise typed mutation publication validation in real resolver VMs.

use std::sync::Arc;
use std::time::Instant;

use chigiri::core::{Object, Value};

use super::tests::{contract, record, script};
use super::{Execution, ExecutionBudget, VM_TIME, execute_nested_with, execute_root_with};
use crate::cancellation::Cancellation;
use crate::hooks::HookSlot;
use crate::ir::MockIr;
use crate::output::OutputValue;
use crate::publication::{PublicationBuffer, PublicationLimits, PublicationStore};
use crate::store::Store;

const SDL: &str = r#"
type Query { ping: Int! nested: Container! }
type Mutation { scalar: Int! dto: Int! create: Item! fail: Int! }
type Subscription { scalar: Int! dto: Payload! item: Item! }
type Payload { value: String! }
type Container { check: Int! @virtual }
type Item { id: ID! name: String! }
"#;

fn execution(buffer: Option<PublicationBuffer>) -> Execution {
    let budget = ExecutionBudget::default().with_publications(buffer);
    Execution {
        state: Arc::new(Object::new()),
        request: None,
        deadline: Instant::now() + VM_TIME,
        cancel: Cancellation::default(),
        shutdown: Cancellation::default(),
        budget,
    }
}

fn publication_buffer(ir: Arc<MockIr>) -> PublicationBuffer {
    PublicationStore::new(Store::empty(ir), 128)
        .unwrap()
        .capture_owner()
        .buffer(PublicationLimits::default())
        .unwrap()
}

fn publish(coordinate: &str, body: &str) -> Result<OutputValue, crate::luau::ScriptError> {
    let contract = contract(SDL);
    let script = script(&contract, coordinate, body);
    let mut store = Store::empty(contract.ir().clone());
    let snapshot = store.snapshot();
    execute_root_with(
        &script,
        &Object::new(),
        &mut store,
        &snapshot,
        &execution(Some(publication_buffer(contract.ir().clone()))),
    )
}

#[test]
fn mutation_publication_accepts_scalar_dto_and_current_candidate_record_payloads() {
    let _case = crate::vm_gate::test_case();
    for (coordinate, body) in [
        (
            "Mutation.scalar",
            "ctx.pubsub.publish.Int({topic='scalar',payload=7}) return 1",
        ),
        (
            "Mutation.dto",
            "ctx.pubsub.publish.Payload({topic='dto',payload={value='ok'}}) return 2",
        ),
        (
            "Mutation.create",
            "local row=ctx.models.Item:create({id='one',name='current'}) ctx.pubsub.publish.Item({topic='item',payload=row}) return row",
        ),
    ] {
        assert!(publish(coordinate, body).is_ok(), "{coordinate}");
    }
}

#[test]
fn publication_rejects_call_shape_topic_payload_and_forged_record_provenance() {
    let _case = crate::vm_gate::test_case();
    for body in [
        "local d:any=ctx d.pubsub.publish.Int(d.pubsub.publish,{topic='x',payload=1}) return 1",
        "local d:any=ctx d.pubsub.publish.Int({topic='x',payload=1,extra=true}) return 1",
        "local d:any=ctx d.pubsub.publish.Int({topic='',payload=1}) return 1",
        "local d:any=ctx d.pubsub.publish.Int({topic='bad\\n',payload=1}) return 1",
        "local d:any=ctx d.pubsub.publish.Int({topic='x',payload='wrong'}) return 1",
        "local d:any=ctx d.pubsub.publish.Item({topic='x',payload={id='fake',name='fake'}}) return 1",
    ] {
        assert!(publish("Mutation.scalar", body).is_err(), "{body}");
    }
    let oversized = format!(
        "local d:any=ctx d.pubsub.publish.Int({{topic='{}',payload=1}}) return 1",
        "x".repeat(257)
    );
    assert!(publish("Mutation.scalar", &oversized).is_err());
}

#[test]
fn publication_bounds_keys_and_utf8_topic_bytes_before_copy() {
    let _case = crate::vm_gate::test_case();
    for topic in ["x".repeat(256), "é".repeat(128)] {
        let body =
            format!("local d:any=ctx d.pubsub.publish.Int({{topic='{topic}',payload=1}}) return 1");
        assert!(publish("Mutation.scalar", &body).is_ok());
    }
    for topic in ["x".repeat(257), "é".repeat(129), "x".repeat(4096)] {
        let body =
            format!("local d:any=ctx d.pubsub.publish.Int({{topic='{topic}',payload=1}}) return 1");
        assert!(publish("Mutation.scalar", &body).is_err());
    }
    for key in ["x".repeat(8), "x".repeat(4096)] {
        let body = format!("local d:any=ctx d.pubsub.publish.Int({{['{key}']=1}}) return 1");
        assert!(publish("Mutation.scalar", &body).is_err());
    }
}

#[test]
fn caught_failure_is_sticky_and_read_or_nested_contexts_have_no_capability() {
    let _case = crate::vm_gate::test_case();
    assert!(
        publish(
            "Mutation.fail",
            "local p:any=ctx.pubsub.publish.Int local ok=pcall(function() p({topic='x',payload='bad'}) end) assert(not ok) return 1",
        )
        .is_err()
    );

    let contract = contract(SDL);
    let mut store = Store::empty(contract.ir().clone());
    let query = script(
        &contract,
        "Query.ping",
        "local dynamic:any=ctx assert(dynamic.pubsub==nil) return 1",
    );
    let parent_query = script(&contract, "Query.nested", "return {}");
    let nested = script(
        &contract,
        "Container.check",
        "local dynamic:any=ctx assert(dynamic.pubsub==nil) return 1",
    );
    let snapshot = store.snapshot();
    let execution = execution(Some(publication_buffer(contract.ir().clone())));
    let query_result = execute_root_with(&query, &Object::new(), &mut store, &snapshot, &execution);
    assert!(
        query_result.is_ok(),
        "query resolution must remain read-only: {:?}",
        query_result.as_ref().err()
    );
    let parent = record(
        execute_root_with(
            &parent_query,
            &Object::new(),
            &mut store,
            &snapshot,
            &execution,
        )
        .expect("prepared parent query must return its container"),
    );
    let nested_result = execute_nested_with(&nested, &Object::new(), &parent, &execution);
    assert!(
        nested_result.is_ok(),
        "nested resolution must remain read-only: {:?}",
        nested_result.as_ref().err()
    );
}

#[test]
fn isolated_mutation_without_request_collector_has_no_publication_capability() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(SDL);
    let script = script(
        &contract,
        "Mutation.scalar",
        "local dynamic:any=ctx assert(dynamic.pubsub==nil) return 1",
    );
    let mut store = Store::empty(contract.ir().clone());
    let snapshot = store.snapshot();
    let output = execute_root_with(
        &script,
        &Object::new(),
        &mut store,
        &snapshot,
        &execution(None),
    )
    .unwrap();
    assert!(matches!(output, OutputValue::Scalar(Value::Int(1))));
}

#[test]
fn hook_context_omits_publication_even_when_its_shared_budget_carries_the_buffer() {
    let _case = crate::vm_gate::test_case();
    let contract = super::hook_tests::contract();
    let slot = HookSlot::RequestBefore;
    let script = super::hook_tests::script(
        &contract,
        slot,
        "local dynamic:any=ctx assert(dynamic.pubsub==nil) return nil",
    );
    let mut call = super::hook_tests::call(&contract, slot);
    call.budget = call
        .budget
        .with_publications(Some(publication_buffer(contract.ir().clone())));
    assert!(super::execute_hook(&script, call).is_ok());
}
