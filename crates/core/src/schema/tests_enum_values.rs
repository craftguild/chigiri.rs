use super::{SCHEMA_RESERVED_ENUM_VALUE, compose_schema};
use crate::language::ast::{Definition, TypeSystemDefinition};
use crate::language::parse_type_system_document;
use crate::{ExecutionLimits, SourceId};

fn parse(source: &str) -> crate::language::ast::Document {
    parse_type_system_document(SourceId::new(90), source, &ExecutionLimits::unbounded())
        .document
        .unwrap()
}

#[test]
fn rejects_reserved_enum_values_in_definitions_extensions_and_public_ast() {
    let mut document =
        parse("enum State { ONE TWO } extend enum State { THREE } type Query { state: State }");
    let mut replacements = ["true", "false", "null"].into_iter();
    for definition in &mut document.definitions {
        match definition {
            Definition::TypeSystem(TypeSystemDefinition::Enum(enumeration)) => {
                for value in &mut enumeration.values {
                    value.name.value = replacements.next().unwrap().to_owned();
                }
            }
            Definition::TypeSystem(TypeSystemDefinition::EnumExtension(extension)) => {
                for value in &mut extension.values {
                    value.name.value = replacements.next().unwrap().to_owned();
                }
            }
            _ => {}
        }
    }
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_RESERVED_ENUM_VALUE)
            .count(),
        3
    );
    assert!(output.schema.is_none());
}

#[test]
fn enum_values_are_not_reserved_literals() {
    let document = parse("enum State { TRUE False Null _null } type Query { state: State }");
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}
