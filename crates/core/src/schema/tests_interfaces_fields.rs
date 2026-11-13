use super::{
    SCHEMA_INTERFACE_ARGUMENT_MISSING, SCHEMA_INTERFACE_ARGUMENT_TYPE_MISMATCH,
    SCHEMA_INTERFACE_DEPRECATION_MISMATCH, SCHEMA_INTERFACE_EXTRA_REQUIRED_ARGUMENT,
    SCHEMA_INTERFACE_FIELD_MISSING, SCHEMA_INTERFACE_RETURN_TYPE_MISMATCH,
    SCHEMA_INVALID_INTERFACE_TARGET, compose_schema,
};
use crate::language::parse_type_system_document;
use crate::{ExecutionLimits, SourceId};

fn parse(id: usize, source: &str) -> crate::language::ast::Document {
    let output =
        parse_type_system_document(SourceId::new(id), source, &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    output.document.unwrap()
}

#[test]
fn accepts_covariant_returns_optional_extra_arguments_and_matching_deprecation() {
    let document = parse(
        90,
        r#"
        interface Node { id: ID! }
        type Image implements Node { id: ID! }
        union Search = Image
        interface Resource implements Node {
          id: ID!
          related(filter: String): [[Node]]
          search: Search
          old: String @deprecated
        }
        interface Factory { resource: Node }
        type Query implements Resource & Node & Factory {
          id: ID!
          related(filter: String, limit: Int! = 10): [[Image!]!]!
          search: Image
          resource: Resource
          old: String @deprecated
        }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}

#[test]
fn missing_interface_fields_report_the_implementor_location() {
    let document = parse(
        91,
        "interface Node { id: ID! } type Query implements Node { ok: String }",
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let diagnostics = output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == SCHEMA_INTERFACE_FIELD_MISSING)
        .collect::<Vec<_>>();

    assert!(output.schema.is_none());
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].span.is_some());
}

#[test]
fn distinguishes_missing_mismatched_and_extra_required_interface_arguments() {
    let document = parse(
        92,
        r#"
        interface Search {
          find(term: String, limit: [Int!]!): String
        }
        type Query implements Search {
          find(
            limit: [Int]!
            required: Int!
            optional: Int
            withDefault: Int! = 1
          ): String
        }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let codes = output
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect::<Vec<_>>();

    assert!(output.schema.is_none());
    assert_eq!(
        codes
            .iter()
            .filter(|code| **code == SCHEMA_INTERFACE_ARGUMENT_MISSING)
            .count(),
        1
    );
    assert_eq!(
        codes
            .iter()
            .filter(|code| **code == SCHEMA_INTERFACE_ARGUMENT_TYPE_MISMATCH)
            .count(),
        1
    );
    assert_eq!(
        codes
            .iter()
            .filter(|code| **code == SCHEMA_INTERFACE_EXTRA_REQUIRED_ARGUMENT)
            .count(),
        1
    );
}

#[test]
fn rejects_invalid_non_null_list_and_named_return_subtypes() {
    let document = parse(
        93,
        r#"
        interface Node { id: ID }
        type Image implements Node { id: ID }
        type Other { id: ID }
        union Search = Image
        interface Contract {
          requiredNode: Node!
          nodeList: [Node]
          node: Node
          search: Search
        }
        type Query implements Contract {
          requiredNode: Node
          nodeList: Node
          node: Other
          search: Other
        }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let diagnostics = output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == SCHEMA_INTERFACE_RETURN_TYPE_MISMATCH)
        .collect::<Vec<_>>();

    assert!(output.schema.is_none());
    assert_eq!(diagnostics.len(), 4);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.span.is_some())
    );
}

#[test]
fn rejects_a_deprecated_implementation_for_a_current_interface_field() {
    let source = concat!(
        "interface Current { value: String } ",
        "type Query implements Current { value: String @deprecated }",
    );
    let document = parse(94, source);
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let diagnostic = output
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == SCHEMA_INTERFACE_DEPRECATION_MISMATCH)
        .unwrap();

    assert!(output.schema.is_none());
    assert_eq!(
        diagnostic.span.unwrap().start,
        source.find("@deprecated").unwrap()
    );
}

#[test]
fn interface_extension_fields_are_checked_against_existing_implementors() {
    let document = parse(
        95,
        r#"
        interface Node { id: ID }
        interface Resource implements Node { id: ID }
        type User implements Node { id: ID }
        type Query { user: User }
        extend interface Node { name: String }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.schema.is_none());
    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_INTERFACE_FIELD_MISSING)
            .count(),
        2
    );
}

#[test]
fn invalid_interface_targets_do_not_add_field_cascade_diagnostics() {
    let document = parse(
        96,
        "scalar Token type Query implements Missing & Token { ok: String }",
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_INVALID_INTERFACE_TARGET)
            .count(),
        2
    );
    assert!(output.diagnostics.iter().all(|diagnostic| {
        !matches!(
            diagnostic.code,
            SCHEMA_INTERFACE_FIELD_MISSING
                | SCHEMA_INTERFACE_ARGUMENT_MISSING
                | SCHEMA_INTERFACE_ARGUMENT_TYPE_MISMATCH
                | SCHEMA_INTERFACE_EXTRA_REQUIRED_ARGUMENT
                | SCHEMA_INTERFACE_RETURN_TYPE_MISMATCH
                | SCHEMA_INTERFACE_DEPRECATION_MISMATCH
        )
    }));
}

#[test]
fn field_compatibility_diagnostics_stop_at_the_limit_without_returning_a_schema() {
    let document = parse(
        97,
        r#"
        interface Contract { first: String, second: String, third: String }
        type Query implements Contract { ok: String }
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
            .all(|diagnostic| diagnostic.code == SCHEMA_INTERFACE_FIELD_MISSING)
    );
}

#[test]
fn compares_multiple_interface_arguments_with_both_indexes() {
    const ARGUMENTS: usize = 5_000;
    let mut source = String::with_capacity(ARGUMENTS * 40);
    source.push_str("interface Contract { value(");
    for index in 0..ARGUMENTS {
        source.push_str(&format!("a{index}: Int "));
    }
    source.push_str("): String }\ntype Query implements Contract { value(");
    for index in 0..ARGUMENTS {
        source.push_str(&format!("a{index}: Int "));
    }
    source.push_str("): String }");
    let document = parse(98, &source);
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}

#[test]
fn validates_covariance_for_large_unions_with_a_membership_index() {
    const MEMBERS: usize = 4_000;
    const FIELDS: usize = 4_000;
    let mut source = String::with_capacity((MEMBERS + FIELDS) * 50);
    for index in 0..MEMBERS {
        source.push_str(&format!("type O{index} {{ id: ID }}\n"));
    }
    source.push_str("union Result = ");
    for index in 0..MEMBERS {
        if index != 0 {
            source.push_str(" | ");
        }
        source.push_str(&format!("O{index}"));
    }
    source.push_str("\ninterface Contract {\n");
    for index in 0..FIELDS {
        source.push_str(&format!("f{index}: Result\n"));
    }
    source.push_str("}\ntype Query implements Contract {\n");
    for index in 0..FIELDS {
        source.push_str(&format!("f{index}: O{}\n", MEMBERS - 1));
    }
    source.push('}');
    let document = parse(99, &source);
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}
