use super::{
    SCHEMA_INPUT_DEFAULT_CYCLE, SCHEMA_INVALID_INPUT_TYPE, SCHEMA_UNKNOWN_TYPE, compose_schema,
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
fn accepts_explicit_null_empty_lists_and_shared_branches() {
    let document = parse(
        110,
        r#"
        input Explicit { next: Explicit = { next: null } }
        input EmptyList { children: [EmptyList] = [] }
        input Nullable { next: Nullable }
        input Leaf { value: String }
        input Shared { leaf: Leaf = {} }
        input Parent { left: Shared = {}, right: Shared = {} }
        type Query { ok: String }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}

#[test]
fn rejects_direct_default_cycles_at_the_closing_default_span() {
    let source = "input A { next: A = {} } type Query { ok: String }";
    let document = parse(111, source);
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let diagnostic = output
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == SCHEMA_INPUT_DEFAULT_CYCLE)
        .unwrap();
    let start = source.find("= {}").unwrap() + 2;

    assert!(output.schema.is_none());
    assert_eq!(
        diagnostic.span,
        Some(Span::new(SourceId::new(111), start, start + 2))
    );
}

#[test]
fn indirect_cycles_diagnose_each_closing_default() {
    let source = concat!(
        "input A { b: B = {} } ",
        "input B { a: A = {} } ",
        "type Query { ok: String }",
    );
    let document = parse(112, source);
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let spans = output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == SCHEMA_INPUT_DEFAULT_CYCLE)
        .map(|diagnostic| diagnostic.span.unwrap())
        .collect::<Vec<_>>();
    let first = source.find("{}").unwrap();
    let second = source.rfind("{}").unwrap();

    assert!(output.schema.is_none());
    assert_eq!(spans.len(), 2);
    assert!(spans.contains(&Span::new(SourceId::new(112), first, first + 2)));
    assert!(spans.contains(&Span::new(SourceId::new(112), second, second + 2,)));
}

#[test]
fn empty_object_list_items_can_form_default_cycles() {
    let source = "input A { items: [A] = [{}] } type Query { ok: String }";
    let document = parse(113, source);
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let diagnostic = output
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == SCHEMA_INPUT_DEFAULT_CYCLE)
        .unwrap();
    let start = source.find("[{}]").unwrap();

    assert!(output.schema.is_none());
    assert_eq!(
        diagnostic.span,
        Some(Span::new(SourceId::new(113), start, start + 4))
    );
}

#[test]
fn extension_default_fields_participate_in_the_final_merged_cycle() {
    let extension_source = "extend input A { next: A = {} }";
    let extension = parse(114, extension_source);
    let definitions = parse(115, "input A { value: String } type Query { ok: String }");
    let output = compose_schema(&[extension, definitions], &ExecutionLimits::unbounded());
    let diagnostic = output
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == SCHEMA_INPUT_DEFAULT_CYCLE)
        .unwrap();
    let start = extension_source.find("{}").unwrap();

    assert!(output.schema.is_none());
    assert_eq!(
        diagnostic.span,
        Some(Span::new(SourceId::new(114), start, start + 2))
    );
}

#[test]
fn unknown_and_non_input_default_cycle_references_do_not_add_cascade_diagnostics() {
    let document = parse(
        116,
        "input Broken { missing: Missing = {}, wrong: Query = {} } type Query { ok: String }",
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
            .all(|diagnostic| diagnostic.code != SCHEMA_INPUT_DEFAULT_CYCLE)
    );
}

#[test]
fn detects_long_default_cycles_with_a_worklist() {
    const INPUTS: usize = 20_000;
    let mut source = String::with_capacity(INPUTS * 36);
    for index in 0..INPUTS {
        let next = (index + 1) % INPUTS;
        source.push_str(&format!("input I{index} {{ next: I{next} = {{}} }}\n"));
    }
    source.push_str("type Query { ok: String }");
    let document = parse(117, &source);
    let mut limits = ExecutionLimits::unbounded();
    limits.max_diagnostics = 1;
    let output = compose_schema(&[document], &limits);

    assert!(output.schema.is_none());
    assert_eq!(output.diagnostics.len(), 1);
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        SCHEMA_INPUT_DEFAULT_CYCLE
    );
    assert!(output.diagnostics_were_truncated());
}

#[test]
fn default_cycle_detection_stops_at_the_diagnostic_limit() {
    let document = parse(
        118,
        concat!(
            "input A { next: A = {} } ",
            "input B { next: B = {} } ",
            "input C { next: C = {} } ",
            "type Query { ok: String }",
        ),
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
            .all(|diagnostic| diagnostic.code == SCHEMA_INPUT_DEFAULT_CYCLE)
    );
}

#[test]
fn deep_branching_acyclic_default_graph_is_valid() {
    const DEPTH: usize = 64;
    let mut source = String::with_capacity(DEPTH * 64);
    for index in 0..DEPTH {
        let next = index + 1;
        source.push_str(&format!(
            "input I{index} {{ left: I{next} = {{}}, right: I{next} = {{}} }}\n"
        ));
    }
    source.push_str(&format!("input I{DEPTH} {{ value: String }}\n"));
    source.push_str("type Query { ok: String }");
    let document = parse(119, &source);
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}

#[test]
fn dense_default_cycles_share_edge_ranges() {
    const FIELDS: usize = 10_000;
    let mut source = String::with_capacity(FIELDS * 20);
    source.push_str("input Dense {\n");
    for index in 0..FIELDS {
        source.push_str(&format!("f{index}: Dense = {{}}\n"));
    }
    source.push_str("}\ntype Query { ok: String }");
    let document = parse(120, &source);
    let mut limits = ExecutionLimits::unbounded();
    limits.max_diagnostics = 1;
    let output = compose_schema(&[document], &limits);

    assert!(output.schema.is_none());
    assert_eq!(output.diagnostics.len(), 1);
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        SCHEMA_INPUT_DEFAULT_CYCLE
    );
}
