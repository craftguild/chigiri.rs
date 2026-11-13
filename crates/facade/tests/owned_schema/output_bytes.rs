//! Verify type-name, response-key, scalar, and pre-execution metadata accounting.

use std::fmt::Write;
use std::sync::atomic::Ordering;

use chigiri::{GraphQlRequest, Value};

use super::support::{Dispatch, run, schema};

#[test]
fn rsm_16_introspection_arguments_share_request_input_budget() {
    let source = schema("type Query{value:Int}");
    let dispatch = Dispatch::new(&source, "Query", Value::Int(1));
    let endpoint = chigiri::Endpoint::builder(source.bind(dispatch).unwrap())
        .without_context()
        .inject(|components| {
            components
                .execution()
                .dangerously_enable_introspection("owned input budget test");
        })
        .build()
        .unwrap();
    let fields = numbered(32, "a", ":__type(name:$n){name} ");
    let mut request = GraphQlRequest::new(format!("query($n:String!){{{fields}}}"));
    request
        .variables
        .insert("n", Value::String("x".repeat(1024 * 1024)));
    let response =
        super::support::block_on(endpoint.execute(request, (), &super::support::NoDeadline));
    assert!(!response.errors.is_empty());
    assert!(
        response
            .errors
            .iter()
            .all(|error| error.message.contains("input coercion resource limit"))
    );
    assert!(
        run(&endpoint, "{__type(name:\"Query\"){name}}")
            .errors
            .is_empty()
    );
}

#[test]
fn rsm_16_owned_response_bytes_bound_typenames_alias_keys_and_leaf_values() {
    let name = "N".repeat(20_000);
    let long_alias = "a".repeat(10_000);
    for (ty, alias, value, rows) in [
        (name.as_str(), "t", Value::Int(1), 1000),
        ("Item", long_alias.as_str(), Value::Int(1), 1000),
        ("Item", "v", Value::String("x".repeat(20_000)), 1000),
    ] {
        let scalar = if matches!(value, Value::Int(_)) {
            "Int"
        } else {
            "String"
        };
        let source = schema(&format!(
            "type Query {{ items: [{ty}!]! }} type {ty} {{ value: {scalar} }}"
        ));
        let mut dispatch = Dispatch::new(&source, ty, value);
        dispatch.rows = Some(rows);
        let endpoint = dispatch.endpoint();
        let small = if alias == "v" {
            "{items{__typename}}"
        } else {
            "{items{value}}"
        };
        assert!(run(&endpoint, small).errors.is_empty());
        let field = if alias == "t" { "__typename" } else { "value" };
        let result = run(&endpoint, &format!("{{items{{{alias}:{field}}}}}"));
        assert_eq!(result.data, Some(Value::Null));
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].path.is_empty());
        assert!(result.errors[0].locations.is_empty());
        assert!(!result.errors.is_empty(), "expected shared byte rejection");
        assert!(
            result
                .errors
                .iter()
                .any(|error| error.extensions.get("code").and_then(Value::as_str)
                    == Some("INTERNAL_SERVER_ERROR"))
        );
        assert!(run(&endpoint, "{__typename}").errors.is_empty());
    }
}

#[test]
fn rsm_16_owned_cost_metadata_is_bounded_before_dispatch_and_offline_catalog() {
    let name = "N".repeat(20_000);
    let fields = numbered(999, "f", ":Int ");
    let source = schema(&format!("type Query{{item:{name}}}type {name}{{{fields}}}"));
    let query = format!("{{item{{{}}}}}", numbered(999, "f", " "));
    let dispatch = Dispatch::new(&source, &name, Value::Int(1));
    let calls = dispatch.calls.clone();
    let endpoint = dispatch.endpoint();
    let rejected = run(&endpoint, &query);
    assert!(rejected.data.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        rejected.errors[0]
            .extensions
            .get("code")
            .and_then(Value::as_str),
        Some("BAD_USER_INPUT")
    );
    let dispatch = Dispatch::new(&source, &name, Value::Int(1));
    let executable = source.clone().bind(dispatch).unwrap().into_executable();
    assert!(
        executable
            .validate_operation_metadata(&GraphQlRequest::new(query))
            .is_err()
    );
    assert!(
        executable
            .validate_operation_metadata(&GraphQlRequest::new("{__typename}"))
            .is_ok()
    );
}

#[test]
fn rsm_16_inherited_directive_names_share_storage_and_public_metadata_is_bounded() {
    let fragment = "F".repeat(20_000);
    let source = schema(
        "directive @d repeatable on FRAGMENT_DEFINITION type Query{item:Item} type Item{v:Int}",
    );
    let dispatch = Dispatch::new(&source, "Item", Value::Int(1));
    let calls = dispatch.calls.clone();
    let endpoint = dispatch.endpoint();
    // 64 children × 16 directives. Even the previous deep-clone implementation stayed below 20 MiB.
    let query = format!(
        "{{item{{...{fragment}}}}} fragment {fragment} on Item {} {{{}}}",
        "@d ".repeat(16),
        numbered(64, "a", ":v ")
    );
    let rejected = run(&endpoint, &query);
    assert!(!rejected.errors.is_empty());
    assert_eq!(rejected.data, Some(Value::Null));
    assert_eq!(rejected.errors.len(), 1);
    assert!(rejected.errors[0].path.is_empty());
    assert!(rejected.errors[0].locations.is_empty());
    assert!(calls.load(Ordering::SeqCst) < 65);
    assert!(run(&endpoint, "{item{v}}").errors.is_empty());
}

#[test]
fn rsm_16_owned_introspection_description_and_default_render_share_the_byte_limit() {
    let description = "x".repeat(20_000);
    let source = schema(&format!(
        "type Query{{item:Item}} \"{description}\" type Item{{v(arg:String=\"{description}\"):Int}}"
    ));
    let dispatch = Dispatch::new(&source, "Item", Value::Int(1));
    let endpoint = chigiri::Endpoint::builder(source.bind(dispatch).unwrap())
        .without_context()
        .inject(|components| {
            components.execution().limits(chigiri::ExecutionLimits {
                max_aliases: 1000,
                ..chigiri::ExecutionLimits::production()
            });
            components
                .execution()
                .dangerously_enable_introspection("owned schema resource test");
        })
        .build()
        .unwrap();
    let fields = numbered(999, "d", ":description ");
    let defaults = numbered(995, "d", ":defaultValue ");
    for query in [
        format!("{{__type(name:\"Item\"){{{fields}}}}}"),
        format!("{{__type(name:\"Item\"){{fields{{args{{{defaults}}}}}}}}}"),
    ] {
        let result = run(&endpoint, &query);
        assert_eq!(
            result.data,
            Some(Value::Null),
            "{:?}",
            result
                .errors
                .iter()
                .map(|error| &error.message)
                .collect::<Vec<_>>()
        );
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].path.is_empty());
        assert!(result.errors[0].locations.is_empty());
    }
    assert!(
        run(
            &endpoint,
            "{__type(name:\"Item\"){name description fields{args{defaultValue}}}}"
        )
        .errors
        .is_empty()
    );
}

#[test]
fn rsm_16_input_exhaustion_stops_repeated_directive_error_accumulation() {
    let arguments = numbered(1000, "x", ":Boolean=false ");
    let source = schema(&format!(
        "directive @d({arguments}) repeatable on FIELD type Query{{value:Int}}"
    ));
    let dispatch = Dispatch::new(&source, "Query", Value::Int(1));
    let calls = dispatch.calls.clone();
    let endpoint = dispatch.endpoint();
    // The error path is kept short and the old implementation checks for 100,000 errors.
    let query = format!("{{a:value {}}}", "@d ".repeat(100));
    let result = run(&endpoint, &query);
    assert_eq!(result.errors.len(), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(run(&endpoint, "{__typename}").errors.is_empty());
}

fn numbered(count: usize, prefix: &str, suffix: &str) -> String {
    let mut text = String::new();
    for index in 0..count {
        write!(text, "{prefix}{index}{suffix}").unwrap();
    }
    text
}
