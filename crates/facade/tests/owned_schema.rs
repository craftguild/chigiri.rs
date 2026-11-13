//! RSM-20: owned-input endpoint graph and instance-isolation behavior.

#[path = "owned_schema/budgets.rs"]
mod budgets;
#[path = "owned_schema/context_views.rs"]
mod context_views;
#[path = "owned_schema/output_bytes.rs"]
mod output_bytes;
#[path = "owned_schema/scalars.rs"]
mod scalars;
#[path = "owned_schema/support.rs"]
mod support;
#[path = "owned_schema/validation.rs"]
mod validation;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use chigiri::{SchemaSourceKind, Value};
use support::{Dispatch, run, schema};

#[test]
fn rsm_20_two_schemas_share_executor_but_never_document_cache_or_identity() {
    let a = schema("type Query { item: Item! } type Item { value: Int!, onlyA: Int! }");
    let b =
        schema("schema { query: Entry } type Entry { item: Item! } type Item { value: String! }");
    let endpoint_a = Dispatch::new(&a, "Item", Value::Int(7)).endpoint();
    let endpoint_b = Dispatch::new(&b, "Item", Value::String("second".into())).endpoint();
    assert_ne!(
        endpoint_a.schema_info().schema_digest(),
        endpoint_b.schema_info().schema_digest()
    );
    assert_eq!(endpoint_a.schema_info().generated_abi(), None);
    assert_eq!(
        endpoint_b.schema_info().source_kind(),
        SchemaSourceKind::Owned
    );
    for _ in 0..3 {
        let query = "{ alias: item { ...Fields } } fragment Fields on Item { value }";
        let a = run(&endpoint_a, query);
        let b = run(&endpoint_b, query);
        assert!(a.errors.is_empty(), "{:?}", a.errors);
        assert!(b.errors.is_empty(), "{:?}", b.errors);
        assert_eq!(
            a.data
                .unwrap()
                .as_object()
                .unwrap()
                .get("alias")
                .unwrap()
                .as_object()
                .unwrap()
                .get("value"),
            Some(&Value::Int(7))
        );
        assert_eq!(
            b.data
                .unwrap()
                .as_object()
                .unwrap()
                .get("alias")
                .unwrap()
                .as_object()
                .unwrap()
                .get("value"),
            Some(&Value::String("second".into()))
        );
        let only_a = "{ item { onlyA } }";
        assert!(run(&endpoint_a, only_a).errors.is_empty());
        let rejected = run(&endpoint_b, only_a);
        assert!(rejected.data.is_none());
        assert!(!rejected.errors.is_empty());
    }
}

#[test]
fn rsm_20_foreign_or_static_object_is_rejected_before_typename_and_nested_dispatch() {
    let sdl = "type Query { item: Item! } type Item { value: Int! }";
    for static_object in [false, true] {
        let first = schema(sdl);
        let second = schema(sdl);
        assert_eq!(first.digest(), second.digest());
        assert!(!first.same_instance(&second));
        assert!(first.same_instance(&first.clone()));
        let mut dispatch = Dispatch::new(&first, "Item", Value::Int(1));
        dispatch.object_schema = second;
        dispatch.static_object = static_object;
        let calls = Arc::clone(&dispatch.calls);
        let endpoint = dispatch.endpoint();
        let response = run(&endpoint, "{ item { __typename value } }");
        assert_eq!(response.data, Some(Value::Null));
        assert_eq!(response.errors.len(), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            response.errors[0]
                .extensions
                .get("code")
                .and_then(Value::as_str),
            Some("INTERNAL_SERVER_ERROR")
        );
    }
}

#[test]
fn rsm_20_abstract_completion_checks_members_and_owned_concrete_typenames() {
    let source = schema(
        "type Query { node: Node!, result: Search! } interface Node { value: Int! } type Item implements Node { value: Int! } type Other { value: Int! } union Search = Item",
    );
    let endpoint = Dispatch::new(&source, "Item", Value::Int(3)).endpoint();
    let response = run(
        &endpoint,
        "{ node { __typename value } result { ... on Item { value } } }",
    );
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    let wrong = Dispatch::new(&source, "Other", Value::Int(3)).endpoint();
    assert!(!run(&wrong, "{ node { value } }").errors.is_empty());
    assert!(!run(&wrong, "{ result { __typename } }").errors.is_empty());
    assert!(source.object("Node", ()).is_err());
    assert!(source.object("Search", ()).is_err());
    assert!(source.object("Unknown", ()).is_err());
}

#[test]
fn rsm_20_drop_releases_dispatch_without_global_schema_registry() {
    let source = schema("type Query { item: Item! } type Item { value: Int! }");
    let dispatch = Dispatch::new(&source, "Item", Value::Int(9));
    let probe = Arc::downgrade(&dispatch.lifetime);
    let endpoint = dispatch.endpoint();
    let clone = endpoint.clone();
    drop(source);
    drop(endpoint);
    assert!(probe.upgrade().is_some());
    assert!(run(&clone, "{ item { value } }").errors.is_empty());
    drop(clone);
    assert!(probe.upgrade().is_none());
}

#[test]
fn rsm_20_owned_endpoint_uses_normal_baseline_defaults_and_coercion() {
    let source = schema("type Query { echo(value: Int! = 17): Int! }");
    let endpoint = Dispatch::new(&source, "Query", Value::Int(0)).endpoint();
    assert_eq!(
        endpoint
            .configuration()
            .execution_limits()
            .effective()
            .max_output_nodes,
        100_000
    );
    let hidden = run(&endpoint, "{ __schema { queryType { name } } }");
    assert_eq!(hidden.data, Some(Value::Null));
    assert_eq!(hidden.errors.len(), 1);
    assert_eq!(hidden.errors[0].message, "Schema introspection is disabled");
    let response = run(&endpoint, "{ echo }");
    assert_eq!(
        response.data.unwrap().as_object().unwrap().get("echo"),
        Some(&Value::Int(17))
    );
    assert!(
        run(&endpoint, "query($n: Int! = 23) { echo(value: $n) }")
            .errors
            .is_empty()
    );
    assert!(!run(&endpoint, "{ echo(value: null) }").errors.is_empty());
    assert!(
        !run(&endpoint, "{ echo(value: 2147483648) }")
            .errors
            .is_empty()
    );
}
