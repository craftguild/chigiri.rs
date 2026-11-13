//! Validates `@oneOf` placement and nullable/default constraints for input fields.

use super::super::composer::Composer;
use super::super::{SchemaOrigin, TypeKind};
use crate::language::ast::{Directive, TypeRefKind};

pub(super) fn validate(composer: &mut Composer) {
    for index in 0..composer.schema.types.len() {
        if !composer.can_continue() {
            return;
        }
        let named_type = composer.schema.types[index].clone();
        let one_of = named_type
            .directives
            .iter()
            .zip(named_type.directive_origins.iter().copied())
            .filter(|(directive, _)| directive.name.value == "oneOf")
            .map(|(directive, origin)| (directive.clone(), origin))
            .collect::<Vec<_>>();
        if one_of.is_empty() {
            continue;
        }
        if named_type.kind != TypeKind::InputObject {
            let directives = one_of
                .iter()
                .map(|(directive, _)| directive.clone())
                .collect::<Vec<_>>();
            reject_non_input_targets(composer, &named_type.name, &directives);
            continue;
        }
        let mut declared_on_definition = false;
        for (directive, origin) in one_of {
            if matches!(origin, SchemaOrigin::Definition(_)) {
                declared_on_definition = true;
            } else {
                composer.report(
                    super::super::SCHEMA_ONE_OF_EXTENSION,
                    format!(
                        "Input object `{}` cannot acquire `@oneOf` from an extension",
                        named_type.name
                    ),
                    directive.span,
                );
                if !composer.can_continue() {
                    return;
                }
            }
        }
        if declared_on_definition {
            validate_fields(composer, &named_type);
        }
    }
}

fn reject_non_input_targets(composer: &mut Composer, type_name: &str, directives: &[Directive]) {
    for directive in directives {
        composer.report(
            super::super::SCHEMA_INVALID_ONE_OF_TARGET,
            format!(
                "`@oneOf` may only be applied to an input object definition, not `{type_name}`"
            ),
            directive.span,
        );
        if !composer.can_continue() {
            return;
        }
    }
}

fn validate_fields(composer: &mut Composer, named_type: &super::super::SemanticType) {
    for field in &named_type.input_fields {
        if matches!(field.ty.kind, TypeRefKind::NonNull(_)) {
            composer.report(
                super::super::SCHEMA_ONE_OF_NON_NULL_FIELD,
                format!(
                    "OneOf input field `{}.{}` must be nullable",
                    named_type.name, field.name.value
                ),
                field.ty.span,
            );
            if !composer.can_continue() {
                return;
            }
        }
        if let Some(default) = &field.default_value {
            composer.report(
                super::super::SCHEMA_ONE_OF_DEFAULT_FIELD,
                format!(
                    "OneOf input field `{}.{}` must not define a default value",
                    named_type.name, field.name.value
                ),
                default.span,
            );
        }
        if !composer.can_continue() {
            return;
        }
    }
}
