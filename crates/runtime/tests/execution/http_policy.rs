//! Verify that safe GET-equivalent transport rejects mutations before resolution.

use chigiri_runtime::{GraphQlRequest, Value};

use super::{block_on, schema};

#[test]
fn query_only_policy_allows_queries_and_rejects_mutations_with_method_error() {
    let schema = schema();
    let query = block_on(schema.execute_query_only(GraphQlRequest::new("{ hello }"), ()));
    assert!(query.errors.is_empty());

    let mutation = block_on(schema.execute_query_only(
        GraphQlRequest::new("mutation { set(value: \"changed\") }"),
        (),
    ));
    assert!(mutation.data.is_none());
    assert_eq!(
        mutation.errors[0].extensions.get("code"),
        Some(&Value::from("METHOD_NOT_ALLOWED"))
    );
}
