use super::{
    SCHEMA_INVALID_ONE_OF_TARGET, SCHEMA_ONE_OF_DEFAULT_FIELD, SCHEMA_ONE_OF_EXTENSION,
    SCHEMA_ONE_OF_NON_NULL_FIELD, compose_schema,
};
use crate::language::{ast::Document, parse_type_system_document};
use crate::{ExecutionLimits, SourceId};

fn parse(id: usize, source: &str) -> Document {
    let output =
        parse_type_system_document(SourceId::new(id), source, &ExecutionLimits::unbounded());
    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
    output.document.unwrap()
}

#[test]
fn one_of_definitions_and_extensions_apply_the_same_field_constraints() {
    let document = parse(
        70,
        r#"
        input Choice @oneOf { initial: String!, defaulted: Int = 1 }
        extend input Choice {
          extensionRequired: Boolean!
          extensionDefault: ID = "fallback"
          valid: String
        }
        input Later { value: String }
        extend input Later @oneOf
        type Query @oneOf { value: String }
        scalar Token @oneOf
        enum State @oneOf { READY }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());
    let count = |code| {
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == code)
            .count()
    };

    assert_eq!(count(SCHEMA_INVALID_ONE_OF_TARGET), 3);
    assert_eq!(count(SCHEMA_ONE_OF_EXTENSION), 1);
    assert_eq!(count(SCHEMA_ONE_OF_NON_NULL_FIELD), 2);
    assert_eq!(count(SCHEMA_ONE_OF_DEFAULT_FIELD), 2);
    assert!(output.schema.is_none());
}

#[test]
fn valid_one_of_extensions_require_nullable_fields_without_defaults() {
    let document = parse(
        71,
        r#"
        input CatInput { name: String }
        input DogInput { name: String }
        input PetInput @oneOf { cat: CatInput }
        extend input PetInput { dog: DogInput }
        type Query { find(pet: PetInput!): String }
        "#,
    );
    let output = compose_schema(&[document], &ExecutionLimits::unbounded());

    assert!(output.is_valid(), "{:?}", output.diagnostics.into_vec());
}
