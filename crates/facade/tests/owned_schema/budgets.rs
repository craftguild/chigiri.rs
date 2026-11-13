//! Verifies endpoint rejection of excessive default expansion and variable replication.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use chigiri::Value;
use chigiri::core::Source;
use chigiri::experimental::OwnedSchema;

use super::support::{Dispatch, run, schema};

fn branching(depth: usize) -> String {
    let mut sdl = String::from("type Query { count(input: I0): Int! }");
    for index in 0..depth {
        sdl.push_str(&format!(
            "input I{index} {{ a: I{} = {{}}, b: I{} = {{}} }}",
            index + 1,
            index + 1
        ));
    }
    sdl.push_str(&format!("input I{depth} {{ value: Int = 1 }}"));
    sdl
}

#[test]
fn rsm_16_branching_defaults_are_bounded_before_binding_and_requests_share_budget() {
    let source = schema(&branching(30));
    let dispatch = Dispatch::new(&source, "Query", Value::Int(1));
    assert!(source.clone().bind(dispatch).is_err());
    let source = schema(&branching(10));
    let mut dispatch = Dispatch::new(&source, "Query", Value::Int(1));
    dispatch.ignore_arguments = true;
    let calls = Arc::clone(&dispatch.calls);
    let endpoint = dispatch.endpoint();
    assert!(run(&endpoint, "{ count(input: {}) }").errors.is_empty());
    calls.store(0, Ordering::SeqCst);
    let selections = (0..60)
        .map(|index| format!("f{index}: count(input: {{}})"))
        .collect::<Vec<_>>()
        .join(" ");
    let result = run(&endpoint, &format!("{{ {selections} }}"));
    assert!(
        result
            .errors
            .iter()
            .any(|error| error.message.contains("resource limit"))
    );
    assert!(calls.load(Ordering::SeqCst) < 60);
    assert!(run(&endpoint, "{ count(input: {}) }").errors.is_empty());
    let fields = (0..60)
        .map(|index| format!("f{index}: count(input: $value)"))
        .collect::<Vec<_>>()
        .join(" ");
    let result = run(
        &endpoint,
        &format!("query($value: I0 = {{}}) {{ {fields} }}"),
    );
    assert!(
        result
            .errors
            .iter()
            .any(|error| error.message.contains("resource limit"))
    );
    assert!(run(&endpoint, "{ count(input: {}) }").errors.is_empty());
}

#[test]
fn rsm_16_large_string_default_repetition_is_limited_by_bytes() {
    let sdl = format!(
        "input I {{ value: String = \"{}\" }} input Pair {{ a: I = {{}}, b: I = {{}} }} type Query {{ count(input: Pair = {{}}): Int! }}",
        "x".repeat(110 * 1024)
    );
    assert!(sdl.len() < 128 * 1024);
    let source = schema(&sdl);
    let mut dispatch = Dispatch::new(&source, "Query", Value::Int(1));
    dispatch.ignore_arguments = true;
    let calls = Arc::clone(&dispatch.calls);
    let endpoint = dispatch.endpoint();
    let fields = (0..100)
        .map(|index| format!("f{index}:count"))
        .collect::<Vec<_>>()
        .join(" ");
    let result = run(&endpoint, &format!("{{ {fields} }}"));
    assert!(
        result
            .errors
            .iter()
            .any(|error| error.message.contains("resource limit"))
    );
    assert!(calls.load(Ordering::SeqCst) < 100);
    assert!(run(&endpoint, "{ count }").errors.is_empty());
}

#[test]
fn rsm_16_long_owner_name_is_shared_in_default_cycle_catalog() {
    let owner = "A".repeat(64 * 1024);
    let fields = (0..4000)
        .map(|index| format!("f{index}: L = {{}}"))
        .collect::<Vec<_>>()
        .join(" ");
    let sdl = format!("input {owner} {{ {fields} }} input L {{ x: Int }} type Query {{ q: Int }}");
    assert!(sdl.len() < 128 * 1024);
    let source = OwnedSchema::parse(vec![Source::new("large-name", sdl)]).unwrap();
    assert_eq!(
        source
            .semantic()
            .type_by_name(&owner)
            .unwrap()
            .input_fields()
            .len(),
        4000
    );
}

#[test]
fn rsm_16_list_items_variables_and_directives_cannot_reset_request_budget() {
    let sdl = branching(10).replace("input: I0", "input: [I0]");
    let source = schema(&sdl);
    let mut dispatch = Dispatch::new(&source, "Query", Value::Int(1));
    dispatch.ignore_arguments = true;
    let calls = Arc::clone(&dispatch.calls);
    let endpoint = dispatch.endpoint();
    let items = vec!["{}"; 60].join(",");
    for request in [
        format!("{{ count(input: [{items}]) }}"),
        format!("query($v: [I0] = [{items}]) {{ count(input: $v) }}"),
    ] {
        calls.store(0, Ordering::SeqCst);
        let result = run(&endpoint, &request);
        assert!(
            result
                .errors
                .iter()
                .any(|error| error.message.contains("resource limit"))
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(run(&endpoint, "{ count(input: [{}]) }").errors.is_empty());
    }
    let sdl = branching(10) + " directive @meta(value: I0 = {}) on FIELD";
    let source = schema(&sdl);
    let mut dispatch = Dispatch::new(&source, "Query", Value::Int(1));
    dispatch.ignore_arguments = true;
    let endpoint = dispatch.endpoint();
    let fields = (0..60)
        .map(|index| format!("f{index}:count @meta"))
        .collect::<Vec<_>>()
        .join(" ");
    let result = run(&endpoint, &format!("{{ {fields} }}"));
    assert!(
        result
            .errors
            .iter()
            .any(|error| error.message.contains("resource limit"))
    );
    assert!(run(&endpoint, "{ count @meta }").errors.is_empty());
}

#[test]
fn rsm_16_zero_diagnostics_still_rejects_arguments_and_directives_before_dispatch() {
    let limits = chigiri::ExecutionLimits {
        max_diagnostics: 0,
        ..chigiri::ExecutionLimits::production()
    };
    let items = vec!["{}"; 60].join(",");
    for directive in [false, true] {
        let sdl = branching(10).replace("input: I0", "input: [I0]")
            + " directive @meta(value: [I0]) on FIELD";
        let source = schema(&sdl);
        let mut dispatch = Dispatch::new(&source, "Query", Value::Int(1));
        dispatch.ignore_arguments = true;
        let calls = Arc::clone(&dispatch.calls);
        let endpoint = chigiri::Endpoint::builder(source.bind(dispatch).unwrap())
            .without_context()
            .execution_limits(limits)
            .build()
            .unwrap();
        let request = if directive {
            format!("{{ count @meta(value:[{items}]) }}")
        } else {
            format!("{{ count(input:[{items}]) }}")
        };
        let result = run(&endpoint, &request);
        assert!(!result.errors.is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            result.errors[0]
                .extensions
                .get("code")
                .and_then(Value::as_str),
            Some("BAD_USER_INPUT")
        );
        assert!(run(&endpoint, "{ count @meta }").errors.is_empty());
    }
}
