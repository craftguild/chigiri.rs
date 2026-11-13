use std::sync::atomic::Ordering;

use crate::executor::{
    PreparedSubscriptionOperation, PreparedSubscriptionRoot, ResolvedValue,
    SubscriptionSetupOutcome, execute_subscription_event, prepare_subscription_root,
};
use crate::{ExecutionLimits, FieldError, Location, Object, PathSegment, Value};

use super::fixture::Fixture;
use super::plugin_support::{SessionSetup, StaticEvent};
use super::support::{EventRow, Setup, TestContext, TestDispatch, block_on};

#[test]
fn nullable_and_non_null_root_results_follow_ordinary_completion() {
    let fixture = Fixture::new();
    let dispatch = TestDispatch::new(false);
    let nullable = fixture.operation_for(
        "subscription Nullable {\n  alias: maybe { label }\n}",
        Object::new(),
        ExecutionLimits::unbounded(),
    );
    let root = prepare_alternate(&fixture, &dispatch, &nullable);
    let event = StaticEvent::new(Ok(ResolvedValue::Null));
    let context = TestContext { generation: 2 };
    let plugins = Vec::new();
    let response = block_on(execute_subscription_event(
        fixture.input(&dispatch, &plugins, &context, None),
        &root,
        &event,
    ));
    let mut expected = Object::new();
    expected.insert("alias", Value::Null);
    assert_eq!(response.data, Some(Value::Object(expected)));
    assert!(response.errors.is_empty());

    let prepared = fixture.operation(ExecutionLimits::unbounded());
    let root = prepare_default(&fixture, &dispatch, &prepared);
    let event = StaticEvent::new(Ok(ResolvedValue::Null));
    let response = block_on(execute_subscription_event(
        fixture.input(&dispatch, &plugins, &context, None),
        &root,
        &event,
    ));
    assert_eq!(response.data, Some(Value::Null));
    assert_masked_root_error(&response, "chosen", Location::new(6, 15));
}

#[test]
fn root_resolver_errors_preserve_public_metadata_and_mask_internal_sources() {
    let fixture = Fixture::new();
    let dispatch = TestDispatch::new(false);
    let prepared = fixture.operation(ExecutionLimits::unbounded());
    let root = prepare_default(&fixture, &dispatch, &prepared);
    let context = TestContext { generation: 2 };
    let plugins = Vec::new();
    let public = StaticEvent::new(Err(FieldError::public("event rejected")
        .code("EVENT_REJECTED")
        .extension("reason", "public")));
    let response = block_on(execute_subscription_event(
        fixture.input(&dispatch, &plugins, &context, None),
        &root,
        &public,
    ));
    assert_eq!(response.data, Some(Value::Null));
    let error = &response.errors[0];
    assert_eq!(error.message, "event rejected");
    assert_eq!(error.locations, [Location::new(6, 15)]);
    assert_eq!(error.path, [PathSegment::from("chosen")]);
    assert_eq!(
        error.extensions.get("code").and_then(Value::as_str),
        Some("EVENT_REJECTED")
    );
    assert_eq!(
        error.extensions.get("reason").and_then(Value::as_str),
        Some("public")
    );

    let private = StaticEvent::new(Err(FieldError::internal(PrivateFailure)));
    let response = block_on(execute_subscription_event(
        fixture.input(&dispatch, &plugins, &context, None),
        &root,
        &private,
    ));
    assert_eq!(response.data, Some(Value::Null));
    assert_masked_root_error(&response, "chosen", Location::new(6, 15));
    assert_eq!(response.errors[0].extensions.len(), 1);
}

#[test]
fn list_items_and_abstract_objects_use_the_shared_completion_path() {
    let fixture = Fixture::new();
    let dispatch = TestDispatch::new(false);
    let list = fixture.operation_for(
        "subscription Items {\n  chosen: items { label }\n}",
        Object::new(),
        ExecutionLimits::unbounded(),
    );
    let root = prepare_alternate(&fixture, &dispatch, &list);
    let event = StaticEvent::new(Ok(ResolvedValue::List(vec![
        Ok(ResolvedValue::object(
            "Event",
            EventRow {
                required: "unused",
                label: "first",
            },
        )),
        Err(FieldError::public("item rejected")
            .code("ITEM_REJECTED")
            .extension("item", 1_i64)),
    ])));
    let context = TestContext { generation: 2 };
    let plugins = Vec::new();
    let response = block_on(execute_subscription_event(
        fixture.input(&dispatch, &plugins, &context, None),
        &root,
        &event,
    ));
    let Value::Object(data) = response.data.unwrap() else {
        panic!("expected root object")
    };
    let Value::List(items) = data.get("chosen").unwrap() else {
        panic!("expected completed list")
    };
    let Value::Object(first) = &items[0] else {
        panic!("expected object item")
    };
    assert_eq!(first.get("label").and_then(Value::as_str), Some("first"));
    assert_eq!(items[1], Value::Null);
    let error = &response.errors[0];
    assert_eq!(error.message, "item rejected");
    assert_eq!(error.locations, [Location::new(2, 11)]);
    assert_eq!(
        error.path,
        [PathSegment::from("chosen"), PathSegment::Index(1)]
    );
    assert_eq!(
        error.extensions.get("code").and_then(Value::as_str),
        Some("ITEM_REJECTED")
    );
    assert_eq!(error.extensions.get("item"), Some(&Value::from(1_i64)));

    let abstract_operation = fixture.operation_for(
        "subscription Abstract {\n  node { ... on Event { label } }\n}",
        Object::new(),
        ExecutionLimits::unbounded(),
    );
    let root = prepare_alternate(&fixture, &dispatch, &abstract_operation);
    let event = StaticEvent::new(Ok(ResolvedValue::object(
        "Event",
        EventRow {
            required: "unused",
            label: "abstract",
        },
    )));
    let response = block_on(execute_subscription_event(
        fixture.input(&dispatch, &plugins, &context, None),
        &root,
        &event,
    ));
    let Value::Object(data) = response.data.unwrap() else {
        panic!("expected root object")
    };
    let Value::Object(node) = data.get("node").unwrap() else {
        panic!("expected abstract object")
    };
    assert_eq!(node.get("label").and_then(Value::as_str), Some("abstract"));
    assert!(response.errors.is_empty());
    assert_eq!(dispatch.root_calls.load(Ordering::SeqCst), 0);
    assert_eq!(dispatch.nested_calls.load(Ordering::SeqCst), 2);
}

fn prepare_default(
    fixture: &Fixture,
    dispatch: &TestDispatch,
    operation: &PreparedSubscriptionOperation,
) -> PreparedSubscriptionRoot {
    let setup = Setup::new();
    let plugins = Vec::new();
    let context = TestContext { generation: 1 };
    let outcome = block_on(prepare_subscription_root(
        operation,
        fixture.input(dispatch, &plugins, &context, None),
        &setup,
    ));
    let SubscriptionSetupOutcome::Ready { root, .. } = outcome else {
        panic!("setup failed")
    };
    *root
}

fn prepare_alternate(
    fixture: &Fixture,
    dispatch: &TestDispatch,
    operation: &PreparedSubscriptionOperation,
) -> PreparedSubscriptionRoot {
    let setup = SessionSetup::new("session".to_owned());
    let plugins = Vec::new();
    let context = TestContext { generation: 1 };
    let outcome = block_on(prepare_subscription_root(
        operation,
        fixture.input(dispatch, &plugins, &context, None),
        &setup,
    ));
    let SubscriptionSetupOutcome::Ready { root, .. } = outcome else {
        panic!("setup failed")
    };
    *root
}

fn assert_masked_root_error(
    response: &crate::GraphQlResponse,
    response_key: &str,
    location: Location,
) {
    assert_eq!(response.errors.len(), 1);
    let error = &response.errors[0];
    assert_eq!(error.message, "Internal server error");
    assert_eq!(error.locations, [location]);
    assert_eq!(error.path, [PathSegment::from(response_key)]);
    assert_eq!(
        error.extensions.get("code").and_then(Value::as_str),
        Some("INTERNAL_SERVER_ERROR")
    );
}

#[derive(Debug)]
struct PrivateFailure;

impl std::fmt::Display for PrivateFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("private event detail")
    }
}

impl std::error::Error for PrivateFailure {}
