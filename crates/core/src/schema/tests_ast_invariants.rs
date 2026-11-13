use super::{SCHEMA_AST_NESTING_LIMIT, SCHEMA_INVALID_AST_SYNTAX, compose_schema};
use crate::language::ast::{Definition, TypeRef, TypeRefKind, TypeSystemDefinition, Value};
use crate::language::parse_type_system_document;
use crate::{ExecutionLimits, SourceId};

fn parse(source: &str) -> crate::language::ast::Document {
    let output =
        parse_type_system_document(SourceId::new(180), source, &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    output.document.unwrap()
}

#[test]
fn rejects_invalid_public_ast_names_before_semantic_conversion() {
    let mut document = parse(
        "directive @tag(value: String) on OBJECT\n\
         interface Node { id: ID }\n\
         type Query implements Node @tag(value: \"x\") { id: ID }",
    );
    for definition in &mut document.definitions {
        match definition {
            Definition::TypeSystem(TypeSystemDefinition::Directive(directive)) => {
                directive.name.value = "bad-name".to_owned();
                directive.arguments[0].name.value.clear();
            }
            Definition::TypeSystem(TypeSystemDefinition::Object(object)) => {
                object.implements_interfaces[0].value = "1Node".to_owned();
                object.directives[0].name.value = "invalid directive name".to_owned();
                object.directives[0].arguments[0].name.value = "bad.arg".to_owned();
                object.fields[0].name.value = "bad field".to_owned();
                let TypeRefKind::Named(name) = &mut object.fields[0].ty.kind else {
                    unreachable!();
                };
                name.value = "ID!".to_owned();
            }
            _ => {}
        }
    }
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.schema.is_none());
    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_INVALID_AST_SYNTAX)
            .count(),
        7
    );
    assert!(
        output
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == SCHEMA_INVALID_AST_SYNTAX)
    );
}

#[test]
fn rejects_invalid_numeric_lexemes_without_invoking_custom_scalars() {
    let mut document = parse(
        "scalar JSON\n\
         type Query { value(first: JSON = 1, second: JSON = 1.0): String }",
    );
    let object = document
        .definitions
        .iter_mut()
        .find_map(|definition| match definition {
            Definition::TypeSystem(TypeSystemDefinition::Object(object)) => Some(object),
            _ => None,
        })
        .unwrap();
    object.fields[0].arguments[0]
        .default_value
        .as_mut()
        .unwrap()
        .value = Value::Int("01".to_owned());
    object.fields[0].arguments[1]
        .default_value
        .as_mut()
        .unwrap()
        .value = Value::Float("1.e2".to_owned());
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.schema.is_none());
    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_INVALID_AST_SYNTAX)
            .count(),
        2
    );
}

#[test]
fn rejects_all_type_system_extensions() {
    let mut document = parse(
        "directive @tag on SCHEMA | SCALAR | OBJECT | INTERFACE | UNION | ENUM | INPUT_OBJECT\n\
         schema { query: Query }\n\
         scalar JSON\n\
         interface Node { id: ID }\n\
         type Query { id: ID }\n\
         type Item { id: ID }\n\
         union Result = Item\n\
         enum State { READY }\n\
         input Filter { term: String }\n\
         extend schema @tag\n\
         extend scalar JSON @tag\n\
         extend type Query @tag\n\
         extend interface Node @tag\n\
         extend union Result @tag\n\
         extend enum State @tag\n\
         extend input Filter @tag",
    );
    for definition in &mut document.definitions {
        match definition {
            Definition::TypeSystem(TypeSystemDefinition::SchemaExtension(extension)) => {
                extension.directives.clear();
                extension.root_operations.clear();
            }
            Definition::TypeSystem(TypeSystemDefinition::ScalarExtension(extension)) => {
                extension.directives.clear();
            }
            Definition::TypeSystem(TypeSystemDefinition::ObjectExtension(extension)) => {
                extension.implements_interfaces.clear();
                extension.directives.clear();
                extension.fields.clear();
            }
            Definition::TypeSystem(TypeSystemDefinition::InterfaceExtension(extension)) => {
                extension.implements_interfaces.clear();
                extension.directives.clear();
                extension.fields.clear();
            }
            Definition::TypeSystem(TypeSystemDefinition::UnionExtension(extension)) => {
                extension.directives.clear();
                extension.members.clear();
            }
            Definition::TypeSystem(TypeSystemDefinition::EnumExtension(extension)) => {
                extension.directives.clear();
                extension.values.clear();
            }
            Definition::TypeSystem(TypeSystemDefinition::InputObjectExtension(extension)) => {
                extension.directives.clear();
                extension.fields.clear();
            }
            _ => {}
        }
    }
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.schema.is_none());
    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_INVALID_AST_SYNTAX)
            .count(),
        7
    );
}

#[test]
fn bounds_deep_hand_built_types_and_values_before_cloning() {
    const DEPTH: usize = 10_000;
    let mut document = parse("scalar JSON type Query { value(input: JSON = 1): String }");
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
    for _ in 0..DEPTH {
        let span = argument.ty.span;
        let inner = std::mem::replace(
            &mut argument.ty,
            TypeRef::named(
                crate::language::ast::Name::new("JSON".to_owned(), span),
                span,
            ),
        );
        argument.ty = TypeRef::list(inner, span);
    }
    let default = argument.default_value.as_mut().unwrap();
    for _ in 0..DEPTH {
        let inner = std::mem::replace(&mut default.value, Value::Null);
        default.value = Value::List(vec![crate::language::ast::ValueNode::new(
            inner,
            default.span,
        )]);
    }
    let documents = vec![document];
    let mut limits = ExecutionLimits::production();
    limits.max_parser_nesting = 8;
    let output = compose_schema(&documents, &limits);

    assert!(output.schema.is_none());
    assert_eq!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == SCHEMA_AST_NESTING_LIMIT)
            .count(),
        2
    );
    // Avoid recursively dropping the intentionally deep tree at the end of the test.
    std::mem::forget(documents);
}
