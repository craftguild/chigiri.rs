use std::sync::atomic::Ordering;

use crate::executor::{
    SubscriptionSetupOutcome, execute_subscription_event, prepare_subscription_root,
};
use crate::{ExecutionLimits, Object, Value};

use super::fixture::Fixture;
use super::support::{Event, Setup, TestContext, TestDispatch, block_on};

#[test]
fn event_execution_uses_only_the_operation_package_retained_during_setup() {
    let fixture = Fixture::new();
    let prepared = fixture.operation(ExecutionLimits::unbounded());
    let dispatch = TestDispatch::new(false);
    let plugins = Vec::new();
    let setup_context = TestContext { generation: 1 };
    let setup = Setup::new();
    let outcome = block_on(prepare_subscription_root(
        &prepared,
        fixture.input(&dispatch, &plugins, &setup_context, None),
        &setup,
    ));
    let SubscriptionSetupOutcome::Ready { root, .. } = outcome else {
        panic!("setup failed")
    };

    let mut foreign_variables = Object::new();
    foreign_variables.insert("id", "foreign-room");
    let foreign = fixture.operation_for(
        r#"
            subscription Other($id: ID!) @audit(level: 9) {
              chosen: event(id: $id) { label }
            }
        "#,
        foreign_variables,
        ExecutionLimits::unbounded(),
    );

    // The event API has no parameter through which `foreign` can replace the
    // exact package owned by `root`. Keeping it live proves the packages are
    // independent while the event still executes the retained selection.
    let event = Event::new("retained-required", "retained-label");
    let event_context = TestContext { generation: 2 };
    let response = block_on(execute_subscription_event(
        fixture.input(&dispatch, &plugins, &event_context, None),
        &root,
        &event,
    ));
    drop(foreign);

    let Value::Object(data) = response.data.unwrap() else {
        panic!("expected response object")
    };
    let Value::Object(chosen) = data.get("chosen").unwrap() else {
        panic!("expected retained root selection")
    };
    assert_eq!(
        chosen.get("required").and_then(Value::as_str),
        Some("retained-required")
    );
    assert_eq!(
        chosen.get("label").and_then(Value::as_str),
        Some("retained-label")
    );
    assert!(response.errors.is_empty());
    assert_eq!(root.arguments().get("id"), Some(&Value::from("room")));
    assert_eq!(dispatch.root_calls.load(Ordering::SeqCst), 0);
    assert_eq!(dispatch.nested_calls.load(Ordering::SeqCst), 2);
}
