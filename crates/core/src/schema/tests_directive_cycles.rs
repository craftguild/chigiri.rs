use super::{SCHEMA_DIRECTIVE_DEFINITION_CYCLE, compose_schema};
use crate::language::{ast::Document, parse_type_system_document};
use crate::{ExecutionLimits, SourceId};

fn parse(id: usize, source: &str) -> Document {
    let output =
        parse_type_system_document(SourceId::new(id), source, &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    output.document.unwrap()
}

fn compose(source: &str) -> super::SchemaBuildOutput {
    compose_schema(&[parse(150, source)], &ExecutionLimits::unbounded())
}

#[test]
fn rejects_recursive_directive_definitions() {
    let output = compose(
        "directive @loop(arg: String @loop) on ARGUMENT_DEFINITION\n\
         type Query { value: String }",
    );

    assert_eq!(output.diagnostics.len(), 1);
    assert_eq!(
        output.diagnostics.iter().next().unwrap().code,
        SCHEMA_DIRECTIVE_DEFINITION_CYCLE
    );
    assert!(output.schema.is_none());
}

#[test]
fn rejects_direct_self_referential_directive_cycles() {
    let output = compose(
        "directive @first(arg: String @second) on ARGUMENT_DEFINITION\n\
         directive @second(arg: String @first) on ARGUMENT_DEFINITION\n\
         type Query { value: String }",
    );

    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_DIRECTIVE_DEFINITION_CYCLE)
            .count(),
        1
    );
    assert!(output.schema.is_none());
}

#[test]
fn rejects_a_directive_cycle_back_to_its_input_type() {
    let output = compose(
        "directive @loop(arg: Metadata) on INPUT_OBJECT\n\
         input Metadata @loop { value: String }\n\
         type Query { value: String }",
    );

    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == SCHEMA_DIRECTIVE_DEFINITION_CYCLE)
    );
    assert!(output.schema.is_none());
}

#[test]
fn nullable_input_types_break_directive_cycles() {
    let output = compose(
        "directive @safe(arg: A) on INPUT_OBJECT\n\
         input A { next: B }\n\
         input B { next: A }\n\
         type Query { value: String }",
    );

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}

#[test]
fn scalar_and_enum_values_do_not_form_directive_reference_cycles() {
    let scalar = compose(
        "directive @scalarLoop(arg: Token) on SCALAR\n\
         scalar Token @scalarLoop\n\
         type Query { value: String }",
    );
    let enumeration = compose(
        "directive @enumLoop(arg: Choice) on ENUM_VALUE\n\
         enum Choice { YES @enumLoop }\n\
         type Query { value: String }",
    );

    assert!(scalar.schema.is_none());
    assert!(enumeration.schema.is_none());
    assert!(
        scalar
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == SCHEMA_DIRECTIVE_DEFINITION_CYCLE)
    );
    assert!(
        enumeration
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == SCHEMA_DIRECTIVE_DEFINITION_CYCLE)
    );
}

#[test]
fn long_input_type_paths_do_not_recurse_on_the_call_stack() {
    let count = 20_000;
    let mut source = String::from("directive @loop(arg: T0) on INPUT_OBJECT\n");
    for index in 0..count {
        source.push_str(&format!("input T{index} {{ next: T{} }}\n", index + 1));
    }
    source.push_str(&format!("input T{count} @loop {{ value: String }}\n"));
    source.push_str("type Query { value: String }");

    let output = compose(&source);

    assert!(output.schema.is_none());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == SCHEMA_DIRECTIVE_DEFINITION_CYCLE)
    );
}
