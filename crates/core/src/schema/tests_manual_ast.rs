use super::{
    SCHEMA_DIRECTIVE_LOCATION_MISSING, SCHEMA_INVALID_DIRECTIVE_LOCATION,
    SCHEMA_INVALID_ONE_OF_TARGET, SCHEMA_INVALID_SPECIFIED_BY_TARGET, SCHEMA_INVALID_TYPE_WRAPPING,
    compose_schema,
};
use crate::language::ast::{Definition, TypeRef, TypeSystemDefinition};
use crate::language::parse_type_system_document;
use crate::{ExecutionLimits, SourceId};

fn parse(source: &str) -> crate::language::ast::Document {
    let output =
        parse_type_system_document(SourceId::new(170), source, &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    output.document.unwrap()
}

#[test]
fn rejects_hand_built_ast_directives_without_locations() {
    let mut document = parse(
        "directive @audit on OBJECT\n\
         type Query { value: String }",
    );
    let definition = document
        .definitions
        .iter_mut()
        .find_map(|definition| match definition {
            Definition::TypeSystem(TypeSystemDefinition::Directive(directive)) => Some(directive),
            _ => None,
        })
        .unwrap();
    definition.locations.clear();

    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.schema.is_none());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == SCHEMA_DIRECTIVE_LOCATION_MISSING)
    );
}

#[test]
fn rejects_hand_built_ast_with_double_non_null_wrappers() {
    let mut document = parse(
        "directive @audit(input: String!) on OBJECT\n\
         type Query { value(input: String!): String! }",
    );
    let directive_argument = document
        .definitions
        .iter_mut()
        .find_map(|definition| match definition {
            Definition::TypeSystem(TypeSystemDefinition::Directive(directive)) => {
                Some(&mut directive.arguments[0])
            }
            _ => None,
        })
        .unwrap();
    directive_argument.ty =
        TypeRef::non_null(directive_argument.ty.clone(), directive_argument.ty.span);
    let argument = document
        .definitions
        .iter_mut()
        .find_map(|definition| match definition {
            Definition::TypeSystem(TypeSystemDefinition::Object(object)) => {
                Some(&mut object.fields[0].arguments[0])
            }
            _ => None,
        })
        .unwrap();
    argument.ty = TypeRef::non_null(argument.ty.clone(), argument.ty.span);
    let field = document
        .definitions
        .iter_mut()
        .find_map(|definition| match definition {
            Definition::TypeSystem(TypeSystemDefinition::Object(object)) => {
                Some(&mut object.fields[0])
            }
            _ => None,
        })
        .unwrap();
    field.ty = TypeRef::non_null(field.ty.clone(), field.ty.span);

    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.schema.is_none());
    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_INVALID_TYPE_WRAPPING)
            .count(),
        3
    );
}

#[test]
fn builtin_specific_targets_do_not_duplicate_generic_location_diagnostics() {
    let document = parse(
        "scalar Token @oneOf\n\
         extend scalar String @specifiedBy(url: \"https://example.com\")\n\
         type Query { value: String }",
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_INVALID_ONE_OF_TARGET)
            .count(),
        1
    );
    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_INVALID_SPECIFIED_BY_TARGET)
            .count(),
        1
    );
    assert!(
        !output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == SCHEMA_INVALID_DIRECTIVE_LOCATION)
    );
}
