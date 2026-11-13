use chigiri_runtime::core::Source;
use chigiri_runtime::{OwnedSchema, SchemaBuildError};

use super::fixture::fixture;

fn assert_error<T>(result: Result<T, SchemaBuildError>, expected: &str) {
    match result {
        Ok(_) => panic!("invalid binding was accepted"),
        Err(error) => assert_eq!(error.to_string(), expected),
    }
}

#[test]
fn absent_or_incorrect_subscription_dispatch_root_is_rejected() {
    for root in [None, Some("Subscription".to_owned())] {
        let (schema, mut dispatch, _) = fixture();
        dispatch.root = root;
        assert_error(
            schema.bind_with_subscriptions(dispatch),
            "owned schema and subscription dispatch root do not match",
        );
    }
}

#[test]
fn dispatch_cannot_invent_a_subscription_root() {
    let schema = OwnedSchema::parse(vec![Source::new(
        "finite.graphql",
        "type Query { echo: String }",
    )])
    .unwrap();
    let (_, dispatch, _) = fixture();
    assert_error(
        schema.bind_with_subscriptions(dispatch),
        "owned schema and subscription dispatch root do not match",
    );
}

#[test]
fn subscription_binding_preserves_instance_validation() {
    let (schema, _, _) = fixture();
    let (_, dispatch, _) = fixture();
    assert_error(
        schema.bind_with_subscriptions(dispatch),
        "owned schema and dispatch instances differ",
    );
}

#[test]
fn subscription_binding_preserves_exact_coordinate_validation() {
    for kind in ["missing", "duplicate", "unknown"] {
        let (schema, mut dispatch, _) = fixture();
        match kind {
            "missing" => {
                dispatch.coordinates.pop();
            }
            "duplicate" => dispatch.coordinates[0] = dispatch.coordinates[1].clone(),
            "unknown" => dispatch.coordinates[0].1 = "unknown".into(),
            _ => unreachable!(),
        }
        assert_error(
            schema.bind_with_subscriptions(dispatch),
            "owned resolver coordinates must exactly cover object fields",
        );
    }
}

#[test]
fn subscription_binding_preserves_exact_custom_scalar_validation() {
    let (schema, mut dispatch, _) = fixture();
    dispatch.register_scalar = false;
    assert_error(
        schema.bind_with_subscriptions(dispatch),
        "owned scalar registrations must exactly cover custom scalars",
    );
}
