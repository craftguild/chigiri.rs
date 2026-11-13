use super::{
    SCHEMA_INVALID_INPUT_TYPE, SCHEMA_NON_NULL_INPUT_CYCLE, SCHEMA_UNKNOWN_TYPE, compose_schema,
};
use crate::language::parse_type_system_document;
use crate::{ExecutionLimits, SourceId, Span};

fn parse(id: usize, source: &str) -> crate::language::ast::Document {
    let output =
        parse_type_system_document(SourceId::new(id), source, &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    output.document.unwrap()
}

#[test]
fn accepts_nullable_and_list_cycle_breaks_and_acyclic_shared_children() {
    let document = parse(
        100,
        r#"
        input A { nullableSelf: A, b: B }
        input B { a: A! }
        input C { values: [D!]! }
        input D { c: C! }
        input Top { left: Left!, right: Right! }
        input Left { leaf: Leaf! }
        input Right { leaf: Leaf! }
        input Leaf { value: String! }
        type Query { ok: String }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}

#[test]
fn direct_required_singular_cycles_are_reported_at_the_field_span() {
    let source = "input A { self: A! } type Query { ok: String }";
    let document = parse(101, source);
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let diagnostic = output
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == SCHEMA_NON_NULL_INPUT_CYCLE)
        .unwrap();
    let start = source.find("self: A!").unwrap();

    assert!(output.schema.is_none());
    assert_eq!(
        diagnostic.span,
        Some(Span::new(SourceId::new(101), start, start + 8))
    );
}

#[test]
fn required_singular_fields_reject_indirect_cycles() {
    let source = concat!(
        "input A { b: B! } ",
        "input B { c: C! } ",
        "input C { back: A! } ",
        "type Query { ok: String }",
    );
    let document = parse(102, source);
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let diagnostic = output
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == SCHEMA_NON_NULL_INPUT_CYCLE)
        .unwrap();
    let start = source.find("back: A!").unwrap();

    assert!(output.schema.is_none());
    assert_eq!(
        diagnostic.span,
        Some(Span::new(SourceId::new(102), start, start + 8))
    );
}

#[test]
fn extension_edges_participate_in_cycles_after_final_merge() {
    let extension_source = "extend input B { a: A! }";
    let extension = parse(103, extension_source);
    let definitions = parse(
        104,
        "input A { b: B! } input B { value: String } type Query { ok: String }",
    );
    let output = compose_schema(&[extension, definitions], &ExecutionLimits::unbounded());
    let diagnostic = output
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == SCHEMA_NON_NULL_INPUT_CYCLE)
        .unwrap();
    let start = extension_source.find("a: A!").unwrap();

    assert!(output.schema.is_none());
    assert_eq!(
        diagnostic.span,
        Some(Span::new(SourceId::new(103), start, start + 5))
    );
}

#[test]
fn unknown_and_non_input_cycle_references_do_not_add_cascade_diagnostics() {
    let document = parse(
        105,
        "input Broken { missing: Missing!, wrong: Query! } type Query { ok: String }",
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.schema.is_none());
    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_UNKNOWN_TYPE)
            .count(),
        1
    );
    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_INVALID_INPUT_TYPE)
            .count(),
        1
    );
    assert!(
        output
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != SCHEMA_NON_NULL_INPUT_CYCLE)
    );
}

#[test]
fn detects_long_input_cycles_with_a_bounded_stack() {
    const INPUTS: usize = 20_000;
    let mut source = String::with_capacity(INPUTS * 32);
    for index in 0..INPUTS {
        let next = (index + 1) % INPUTS;
        source.push_str(&format!("input I{index} {{ next: I{next}! }}\n"));
    }
    source.push_str("type Query { ok: String }");
    let document = parse(106, &source);
    let mut limits = ExecutionLimits::unbounded();
    limits.max_diagnostics = 1;
    let output = compose_schema(&[document], &limits);

    assert!(output.schema.is_none());
    assert_eq!(output.diagnostics.len(), 1);
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        SCHEMA_NON_NULL_INPUT_CYCLE
    );
    assert!(output.diagnostics_were_truncated());
}

#[test]
fn input_cycle_detection_stops_at_the_diagnostic_limit() {
    let document = parse(
        107,
        "input A { first: A!, second: A!, third: A! } type Query { ok: String }",
    );
    let mut limits = ExecutionLimits::unbounded();
    limits.max_diagnostics = 2;
    let output = compose_schema(&[document], &limits);

    assert!(output.schema.is_none());
    assert_eq!(output.diagnostics.len(), 2);
    assert!(output.diagnostics_were_truncated());
    assert!(
        output
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == SCHEMA_NON_NULL_INPUT_CYCLE)
    );
}
