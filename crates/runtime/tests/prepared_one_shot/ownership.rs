use std::sync::Arc;

use chigiri_runtime::__private::PreparedOneShotOperation;

use super::fixture::{named, schema};

fn assert_send_static<T: Send + 'static>(_: &T) {}

#[test]
fn prepared_checkpoint_is_send_static_and_releases_its_schema_owners_on_drop() {
    let (schema, calls) = schema();
    let weak = Arc::downgrade(&calls);
    let prepared = schema
        .prepare_one_shot_operation(named("query", "echo", "owned"))
        .unwrap();
    assert_send_static(&prepared);
    drop(schema);
    drop(calls);
    assert!(weak.upgrade().is_some());

    drop(prepared);

    assert!(weak.upgrade().is_none());
}

#[test]
fn retained_charge_counts_both_request_and_coerced_owned_values() {
    let (schema, _) = schema();
    let short = schema
        .prepare_one_shot_operation(named("query", "echo", "a"))
        .unwrap();
    let long = schema
        .prepare_one_shot_operation(named("query", "echo", "abcdef"))
        .unwrap();
    let short_bytes = short.retained_metadata_bytes().unwrap();
    let long_bytes = long.retained_metadata_bytes().unwrap();

    assert_eq!(long_bytes - short_bytes, 10);
    assert!(short_bytes > short.retained_request().query.len());
}

#[allow(dead_code)]
fn checkpoint_type_is_publicly_nameable<C>(_: PreparedOneShotOperation<C>) {}
