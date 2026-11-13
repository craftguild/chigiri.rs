use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::executable::{
    CoercedVariables, CustomScalarRegistry, InputCoercionBudget,
    validate_executable_document_with_scalars,
};
use crate::executor::{
    PreparedSubscriptionOperation, SubscriptionExecutionInput, SubscriptionSetupOutcome,
    establish_subscription_source, prepare_subscription_plan, prepare_subscription_root,
};
use crate::language::{parse_executable_document, parse_type_system_document};
use crate::schema::compose_schema;
use crate::{
    DocumentDigest, ExecutionLimits, Object, OperationIdentity, OperationKey, SchemaDigest, Source,
    SourceId, Value,
};

use super::fixture::Fixture;
use super::plugin_support::SessionSetup;
use super::support::{Setup, TestContext, TestDispatch, block_on};

#[test]
fn context_free_preflight_retains_metadata_for_one_source_initialization() {
    let fixture = Fixture::new();
    let operation = fixture.operation(ExecutionLimits::unbounded());
    let setup = Setup::new();
    let plan = match prepare_subscription_plan(&operation, None, None, None, None) {
        Ok(plan) => plan,
        Err(response) => panic!("preflight failed: {response:?}"),
    };

    assert_eq!(plan.parent_type(), "Subscription");
    assert_eq!(plan.field_name(), "event");
    assert_eq!(plan.response_key(), "chosen");
    assert_eq!(plan.arguments().get("id"), Some(&Value::from("room")));
    assert_eq!(setup.calls.load(Ordering::SeqCst), 0);

    let dispatch = TestDispatch::new(false);
    let plugins = Vec::new();
    let context = TestContext { generation: 1 };
    let outcome = block_on(establish_subscription_source(
        plan,
        fixture.input(&dispatch, &plugins, &context, None),
        &setup,
    ));
    let SubscriptionSetupOutcome::Ready { root, session } = outcome else {
        panic!("source establishment failed")
    };
    assert_eq!(session, "session");
    assert_eq!(setup.calls.load(Ordering::SeqCst), 1);
    assert_eq!(*setup.contexts.lock().unwrap(), [1]);
    assert_eq!(root.arguments().get("id"), Some(&Value::from("room")));
}

#[test]
fn root_coercion_failure_is_returned_before_source_establishment() {
    let fixture = Fixture::new();
    let mut raw = Object::new();
    raw.insert("id", "room");
    let operation = fixture.operation_for(
        "subscription Invalid($id: ID!) { chosen: event(id: $id) { label } }",
        raw,
        ExecutionLimits::unbounded(),
    );
    let budget = InputCoercionBudget::new(0, 0);
    let setup = Setup::new();
    let response = match prepare_subscription_plan(&operation, None, Some(&budget), None, None) {
        Ok(_) => panic!("bounded preflight unexpectedly succeeded"),
        Err(response) => response,
    };
    assert!(response.data.is_none());
    assert!(!response.errors.is_empty());
    assert_eq!(setup.calls.load(Ordering::SeqCst), 0);

    let dispatch = TestDispatch::new(false);
    let plugins = Vec::new();
    let context = TestContext { generation: 1 };
    let budget = InputCoercionBudget::new(0, 0);
    let input = SubscriptionExecutionInput::new(
        &dispatch,
        &plugins,
        &context,
        None,
        None,
        None,
        None,
        Some(&budget),
    );
    let outcome = block_on(prepare_subscription_root(&operation, input, &setup));
    assert!(matches!(outcome, SubscriptionSetupOutcome::Response(_)));
    assert_eq!(setup.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn argument_and_directive_coercion_run_only_during_preflight() {
    let calls = Arc::new(AtomicUsize::new(0));
    let operation = counting_operation(&calls);
    let plan = match prepare_subscription_plan(&operation, None, None, None, None) {
        Ok(plan) => plan,
        Err(response) => panic!("preflight failed: {response:?}"),
    };
    let preflight_calls = calls.load(Ordering::SeqCst);
    assert!(preflight_calls >= 3);
    assert_eq!(
        plan.arguments().get("token"),
        Some(&Value::from("argument"))
    );

    let fixture = Fixture::new();
    let dispatch = TestDispatch::new(false);
    let plugins = Vec::new();
    let context = TestContext { generation: 1 };
    let setup = SessionSetup::new("session".to_owned());
    let outcome = block_on(establish_subscription_source(
        plan,
        fixture.input(&dispatch, &plugins, &context, None),
        &setup,
    ));
    assert!(matches!(outcome, SubscriptionSetupOutcome::Ready { .. }));
    assert_eq!(setup.calls.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), preflight_calls);
}

fn counting_operation(calls: &Arc<AtomicUsize>) -> PreparedSubscriptionOperation {
    let limits = ExecutionLimits::unbounded();
    let parsed_schema = parse_type_system_document(
        SourceId::new(810),
        r#"
            schema { query: Query subscription: Subscription }
            scalar Token
            directive @audit(token: Token!) on SUBSCRIPTION | FIELD
            type Query { ping: String }
            type Subscription { event(token: Token!): Event! }
            type Event { label: String }
        "#,
        &limits,
    );
    let schema = compose_schema(&[parsed_schema.document.unwrap()], &limits)
        .schema
        .unwrap();
    let query = r#"
        subscription @audit(token: "operation") {
          event(token: "argument") @audit(token: "field") { label }
        }
    "#;
    let parsed = parse_executable_document(SourceId::new(811), query, &limits);
    let mut scalars = CustomScalarRegistry::new();
    let scalar_calls = Arc::clone(calls);
    scalars.register("Token", move |_, value| {
        scalar_calls.fetch_add(1, Ordering::SeqCst);
        Ok(value)
    });
    let validated = validate_executable_document_with_scalars(
        &schema,
        &parsed.document.unwrap(),
        &scalars,
        &limits,
    );
    assert!(
        validated.document.is_some(),
        "{:?}",
        validated.diagnostics.into_vec()
    );
    let document = validated.document.unwrap();
    calls.store(0, Ordering::SeqCst);
    PreparedSubscriptionOperation::new(
        Arc::new(schema),
        Arc::new(document),
        None,
        Arc::new(CoercedVariables::new()),
        Arc::new(scalars),
        Arc::new(Source::new("counting.graphql", query)),
        limits,
        false,
        Arc::new(OperationIdentity::new(
            SchemaDigest::from_bytes([3; 32]),
            OperationKey::new(DocumentDigest::from_bytes([4; 32]), None),
        )),
        None,
    )
    .unwrap()
}
