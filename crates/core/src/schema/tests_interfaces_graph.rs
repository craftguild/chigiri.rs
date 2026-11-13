use super::{
    SCHEMA_DUPLICATE_MEMBER, SCHEMA_INTERFACE_CYCLE, SCHEMA_INTERFACE_SELF_IMPLEMENTATION,
    SCHEMA_INVALID_INTERFACE_TARGET, SCHEMA_TRANSITIVE_INTERFACE_MISSING, compose_schema,
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
fn resolves_direct_and_transitive_interfaces_in_the_composed_graph() {
    let extensions = parse(
        80,
        r#"
        extend interface Resource implements Node
        extend interface Image implements Resource & Node
        extend type Query implements Image & Resource & Node
        "#,
    );
    let definitions = parse(
        81,
        r#"
        interface Node { id: ID }
        interface Resource { id: ID }
        interface Image { id: ID }
        type Query { id: ID }
        "#,
    );
    let output = compose_schema(&[extensions, definitions], &ExecutionLimits::unbounded());

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}

#[test]
fn builds_possible_object_indexes_during_composition() {
    let document = parse(
        88,
        r#"
        interface Node { id: ID }
        type User implements Node { id: ID }
        type Photo implements Node { id: ID }
        union Search = Photo | User
        type Query { search: Search }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let schema = output.schema.unwrap();

    assert_eq!(
        schema
            .possible_objects("User")
            .unwrap()
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<&str>>(),
        ["User"]
    );
    assert_eq!(
        schema
            .possible_objects("Node")
            .unwrap()
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<&str>>(),
        ["Photo", "User"]
    );
    assert_eq!(
        schema
            .possible_objects("Search")
            .unwrap()
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<&str>>(),
        ["Photo", "User"]
    );
}

#[test]
fn diagnoses_undefined_targets_kind_mismatches_and_overlapping_interfaces() {
    let document = parse(
        82,
        r#"
        scalar Token
        interface Node { id: ID }
        interface Broken implements Missing & Token & Node & Node { id: ID }
        type Query implements Missing & Token & Node & Node { id: ID }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.schema.is_none());
    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_INVALID_INTERFACE_TARGET)
            .count(),
        4
    );
    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_DUPLICATE_MEMBER)
            .count(),
        2
    );
    assert!(output.diagnostics.iter().all(|diagnostic| {
        if matches!(
            diagnostic.code,
            SCHEMA_INVALID_INTERFACE_TARGET | SCHEMA_DUPLICATE_MEMBER
        ) {
            diagnostic.span.is_some()
        } else {
            true
        }
    }));
}

#[test]
fn rejects_direct_interface_self_implementation() {
    let source = "interface Node implements Node { id: ID } type Query { ok: String }";
    let document = parse(83, source);
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let diagnostic = output
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == SCHEMA_INTERFACE_SELF_IMPLEMENTATION)
        .unwrap();
    let second_node = source.rfind("Node").unwrap();

    assert!(output.schema.is_none());
    assert_eq!(
        diagnostic.span,
        Some(Span::new(SourceId::new(83), second_node, second_node + 4))
    );
}

#[test]
fn interface_cycles_report_the_closing_edge_span_deterministically() {
    let source = concat!(
        "interface A implements B { a: String } ",
        "interface B implements C { b: String } ",
        "interface C implements A { c: String } ",
        "type Query { ok: String }",
    );
    let document = parse(84, source);
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let diagnostic = output
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == SCHEMA_INTERFACE_CYCLE)
        .unwrap();
    let closing_start = source.find("implements A").unwrap() + "implements ".len();

    assert!(output.schema.is_none());
    assert_eq!(
        diagnostic.span,
        Some(Span::new(
            SourceId::new(84),
            closing_start,
            closing_start + 1,
        ))
    );
}

#[test]
fn objects_and_interfaces_include_transitive_interfaces() {
    let document = parse(
        85,
        r#"
        interface Node { id: ID }
        interface Resource implements Node { id: ID }
        interface Image implements Resource & Node { id: ID }
        interface Picture implements Image & Resource { id: ID }
        type Query implements Image & Resource { id: ID }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let diagnostics = output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == SCHEMA_TRANSITIVE_INTERFACE_MISSING)
        .collect::<Vec<_>>();

    assert!(output.schema.is_none());
    // If there are multiple paths to the same interface, we fold them into one by implementor.
    assert_eq!(diagnostics.len(), 2);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.span.is_some())
    );
}

#[test]
fn interface_validation_stops_at_the_diagnostic_limit() {
    let document = parse(
        86,
        r#"
        scalar Token
        interface Broken implements Missing & Token & Broken { id: ID }
        type Query { ok: String }
        "#,
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
            .all(|diagnostic| diagnostic.code == SCHEMA_INVALID_INTERFACE_TARGET)
    );
}

#[test]
fn detects_long_interface_cycles_with_a_bounded_stack() {
    const INTERFACES: usize = 20_000;
    let mut source = String::with_capacity(INTERFACES * 56);
    for index in 0..INTERFACES {
        let next = (index + 1) % INTERFACES;
        source.push_str(&format!(
            "interface I{index} implements I{next} {{ value: String }}\n"
        ));
    }
    source.push_str("type Query { ok: String }");
    let document = parse(87, &source);
    let mut limits = ExecutionLimits::unbounded();
    limits.max_diagnostics = 1;
    let output = compose_schema(&[document], &limits);

    assert!(output.schema.is_none());
    assert_eq!(output.diagnostics.len(), 1);
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        SCHEMA_INTERFACE_CYCLE
    );
    assert!(output.diagnostics_were_truncated());
}

#[test]
fn rsm_16_possible_object_indexes_share_long_names_across_interfaces() {
    let name = "A".repeat(32 * 1024);
    let interfaces = (0..3000)
        .map(|index| format!("I{index}"))
        .collect::<Vec<_>>();
    let mut sdl = format!(
        "type Query {{ q: Int }} type {name} implements {} {{ x: Int }}",
        interfaces.join(" & ")
    );
    for interface in &interfaces {
        sdl.push_str(&format!(" interface {interface} {{x:Int}}"));
    }
    assert!(sdl.len() < 128 * 1024);
    let limits = ExecutionLimits::production();
    let parsed = parse_type_system_document(SourceId::new(0), &sdl, &limits);
    let schema = compose_schema(&[parsed.document.unwrap()], &limits)
        .schema
        .unwrap();
    let own = &schema.possible_objects(&name).unwrap()[0];
    for interface in interfaces {
        let names = schema.possible_objects(&interface).unwrap();
        assert_eq!(names.len(), 1);
        assert!(std::sync::Arc::ptr_eq(own, &names[0]));
    }
}
