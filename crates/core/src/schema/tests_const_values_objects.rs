use super::{
    SCHEMA_CONST_DUPLICATE_INPUT_FIELD, SCHEMA_CONST_ENUM_VALUE, SCHEMA_CONST_INPUT_OBJECT_VALUE,
    SCHEMA_CONST_MISSING_INPUT_FIELD, SCHEMA_CONST_ONE_OF_VALUE, SCHEMA_CONST_SCALAR_VALUE,
    SCHEMA_CONST_UNKNOWN_INPUT_FIELD, compose_schema,
};
use crate::language::parse_type_system_document;
use crate::{DiagnosticCode, ExecutionLimits, SourceId};

fn parse(id: usize, source: &str) -> crate::language::ast::Document {
    let output =
        parse_type_system_document(SourceId::new(id), source, &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    output.document.unwrap()
}

fn count(output: &super::SchemaBuildOutput, code: DiagnosticCode) -> usize {
    output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == code)
        .count()
}

#[test]
fn round_trips_field_interface_input_and_extension_defaults() {
    let extension = parse(
        160,
        concat!(
            "extend type Query { extra(filter: Filter = { nested: { required: 3 } }): String } ",
            "extend input Filter { extension: Nested = { required: 4 } }",
        ),
    );
    let definition = parse(
        161,
        r#"
        enum Color { RED BLUE }
        input Nested { required: Int!, note: String }
        input Filter {
          color: Color = RED
          nested: Nested = { required: 1 }
          values: [Int] = 2
        }
        interface Searchable {
          search(filter: Filter = { color: BLUE, nested: { required: 2 } }): String
        }
        type Query {
          search(filter: Filter = { nested: { required: 2 }, values: [1, 2] }): String
        }
        "#,
    );
    let output = compose_schema(&[extension, definition], &ExecutionLimits::unbounded());

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}

#[test]
fn validates_object_shape_required_fields_nested_values_and_enums() {
    let document = parse(
        162,
        r#"
        enum Color { RED }
        input Inner { required: Int!, known: Int }
        input Outer { inner: Inner!, colors: [Color]! }
        type Query {
          wrongShape(value: Outer = "x"): String
          broken(value: Outer = {
            inner: { required: "x", known: 1, known: 2, extra: 3 }
            colors: ["RED", BLUE]
            unknown: true
          }): String
          missing(value: Outer = { inner: {} }): String
        }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.schema.is_none());
    assert_eq!(count(&output, SCHEMA_CONST_INPUT_OBJECT_VALUE), 1);
    assert_eq!(count(&output, SCHEMA_CONST_DUPLICATE_INPUT_FIELD), 1);
    assert!(count(&output, SCHEMA_CONST_UNKNOWN_INPUT_FIELD) >= 2);
    assert!(count(&output, SCHEMA_CONST_MISSING_INPUT_FIELD) >= 2);
    assert_eq!(count(&output, SCHEMA_CONST_SCALAR_VALUE), 1);
    assert_eq!(count(&output, SCHEMA_CONST_ENUM_VALUE), 2);
}

#[test]
fn one_of_default_is_exactly_one_non_null() {
    let document = parse(
        163,
        r#"
        input Pick @oneOf { text: String, number: Int }
        type Query {
          valid(value: Pick = { text: "x" }): String
          empty(value: Pick = {}): String
          nullMember(value: Pick = { text: null }): String
          multiple(value: Pick = { text: "x", number: 1 }): String
        }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.schema.is_none());
    assert_eq!(count(&output, SCHEMA_CONST_ONE_OF_VALUE), 3);
}

#[test]
fn rejects_invalid_defaults_in_definitions_and_extensions() {
    let extension = parse(
        164,
        concat!(
            "extend input Settings { extensionBad: Int = \"x\" } ",
            "extend type Query { extension(arg: Int = \"x\"): String }",
        ),
    );
    let definition = parse(
        165,
        concat!(
            "input Settings { definitionBad: Int = \"x\" } ",
            "type Query { definition(arg: Int = \"x\"): String }",
        ),
    );
    let output = compose_schema(&[extension, definition], &ExecutionLimits::unbounded());

    assert!(output.schema.is_none());
    assert_eq!(count(&output, SCHEMA_CONST_SCALAR_VALUE), 4);
}

#[test]
fn unknown_types_do_not_add_cascade_diagnostics() {
    let document = parse(
        166,
        concat!(
            "input Broken { unknown: Missing, invalid: Int = \"x\" } ",
            "type Query { ok: String }",
        ),
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.schema.is_none());
    assert_eq!(count(&output, SCHEMA_CONST_SCALAR_VALUE), 0);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == super::SCHEMA_UNKNOWN_TYPE)
    );
}

#[test]
fn validates_large_objects_with_field_lookup() {
    use std::fmt::Write;

    const FIELDS: usize = 10_000;
    let mut source = String::with_capacity(FIELDS * 28);
    source.push_str("input Huge {");
    for index in 0..FIELDS {
        write!(&mut source, "f{index}: Int ").unwrap();
    }
    source.push_str("} type Query { use(value: Huge = {");
    for index in 0..FIELDS {
        write!(&mut source, "f{index}: {index} ").unwrap();
    }
    source.push_str("}): String }");
    let document = parse(167, &source);
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}
