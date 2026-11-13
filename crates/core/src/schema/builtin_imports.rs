//! Reconciles equivalent SDL redeclarations of specification-defined built-ins.

use crate::language::ast::{DirectiveDefinition, TypeRef, TypeRefKind, Value};

use super::{SemanticDirectiveDefinition, SemanticSchema, SemanticTypeRefKind};

pub(super) fn is_equivalent(source: &DirectiveDefinition, schema: &SemanticSchema) -> Option<bool> {
    if !matches!(
        source.name.value.as_str(),
        "skip" | "include" | "deprecated" | "specifiedBy" | "oneOf"
    ) {
        return None;
    }
    let builtin = schema
        .directive_definition(&source.name.value)
        .expect("The target name is GraphQL September 2025 built-in.");
    Some(equivalent_signature(source, builtin))
}

fn equivalent_signature(
    source: &DirectiveDefinition,
    builtin: &SemanticDirectiveDefinition,
) -> bool {
    if source.is_repeatable() != builtin.is_repeatable()
        || source.arguments.len() != builtin.arguments().len()
        || source.locations.len() != builtin.locations().len()
    {
        return false;
    }
    let source_locations = source.locations.iter().fold(0, |mask, location| {
        mask | super::directive_model::location_bit(location.value)
    });
    let builtin_locations = builtin.locations().iter().fold(0, |mask, location| {
        mask | super::directive_model::location_bit(location.value)
    });
    source_locations == builtin_locations
        && source.arguments.iter().all(|argument| {
            argument.directives.is_empty()
                && builtin
                    .argument(&argument.name.value)
                    .is_some_and(|expected| {
                        equivalent_type(&argument.ty, &expected.ty.kind)
                            && equivalent_default(
                                argument.default_value.as_ref().map(|value| &value.value),
                                expected.default_value.as_ref(),
                            )
                    })
        })
}

fn equivalent_type(source: &TypeRef, builtin: &SemanticTypeRefKind) -> bool {
    match (&source.kind, builtin) {
        (TypeRefKind::Named(source), SemanticTypeRefKind::Named(expected)) => {
            source.value == *expected
        }
        (TypeRefKind::List(source), SemanticTypeRefKind::List(expected))
        | (TypeRefKind::NonNull(source), SemanticTypeRefKind::NonNull(expected)) => {
            equivalent_type(source, &expected.kind)
        }
        _ => false,
    }
}

fn equivalent_default(source: Option<&Value>, builtin: Option<&Value>) -> bool {
    match (source, builtin) {
        (None, None) => true,
        (Some(Value::String(source)), Some(Value::String(expected))) => {
            source.value == expected.value
        }
        _ => false,
    }
}
