use crate::executor::{
    SubscriptionSetupOutcome, execute_subscription_event, prepare_subscription_root,
};
use crate::{ExecutionLimits, Value};

use super::fixture::Fixture;
use super::support::{Event, Setup, TestContext, TestDispatch, block_on};

#[test]
fn exact_runtime_bytes_repeat_and_the_next_lower_limit_fails_globally() {
    let fixture = Fixture::new();
    let setup_dispatch = TestDispatch::new(false);
    let plugins = Vec::new();
    let setup_context = TestContext { generation: 1 };
    let setup = Setup::new();
    let operation = fixture.operation(ExecutionLimits::unbounded());
    let outcome = block_on(prepare_subscription_root(
        &operation,
        fixture.input(&setup_dispatch, &plugins, &setup_context, None),
        &setup,
    ));
    let SubscriptionSetupOutcome::Ready { root, .. } = outcome else {
        panic!("setup failed")
    };

    let exact_dispatch = TestDispatch::new(false).with_byte_limit(70);
    for generation in [2, 3] {
        let context = TestContext { generation };
        let event = Event::new("", "");
        let response = block_on(execute_subscription_event(
            fixture.input(&exact_dispatch, &plugins, &context, None),
            &root,
            &event,
        ));
        assert!(response.errors.is_empty(), "{response:?}");
        assert!(matches!(response.data, Some(Value::Object(_))));
    }

    let over_dispatch = TestDispatch::new(false).with_byte_limit(69);
    let context = TestContext { generation: 4 };
    let event = Event::new("", "");
    let response = block_on(execute_subscription_event(
        fixture.input(&over_dispatch, &plugins, &context, None),
        &root,
        &event,
    ));
    assert_eq!(response.data, Some(Value::Null));
    assert_eq!(response.errors.len(), 1);
    let error = &response.errors[0];
    assert_eq!(error.message, "Internal server error");
    assert!(error.path.is_empty());
    assert!(error.locations.is_empty());
    assert_eq!(error.extensions.len(), 1);
    assert_eq!(
        error.extensions.get("code").and_then(Value::as_str),
        Some("INTERNAL_SERVER_ERROR")
    );
}
