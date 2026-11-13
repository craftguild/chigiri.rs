//! Validate `@specifiedBy`, `@deprecated`, and required-input restrictions.

use crate::language::ast::{Directive, InputValueDefinition, TypeRefKind};

use super::super::composer::Composer;
use super::super::{SchemaOrigin, TypeKind};

pub(super) fn validate(composer: &mut Composer) {
    validate_specified_by(composer);
    if composer.can_continue() {
        validate_deprecated_inputs(composer);
    }
}

fn validate_specified_by(composer: &mut Composer) {
    for index in 0..composer.schema.types.len() {
        if !composer.can_continue() {
            return;
        }
        let named_type = composer.schema.types[index].clone();
        let is_custom_scalar = named_type.kind == TypeKind::Scalar
            && !named_type.origins.contains(&SchemaOrigin::BuiltIn);
        if is_custom_scalar {
            continue;
        }
        for directive in named_type
            .directives
            .iter()
            .filter(|directive| directive.name.value == "specifiedBy")
        {
            composer.report(
                super::super::SCHEMA_INVALID_SPECIFIED_BY_TARGET,
                format!(
                    "`@specifiedBy` may only be applied to a custom scalar, not `{}`",
                    named_type.name
                ),
                directive.span,
            );
            if !composer.can_continue() {
                return;
            }
        }
    }
}

fn validate_deprecated_inputs(composer: &mut Composer) {
    for index in 0..composer.schema.types.len() {
        if !composer.can_continue() {
            return;
        }
        let named_type = composer.schema.types[index].clone();
        for field in named_type.fields {
            for argument in field.arguments {
                validate_input_value(
                    composer,
                    &argument,
                    &format!("argument `{}.{}`", field.name.value, argument.name.value),
                );
                if !composer.can_continue() {
                    return;
                }
            }
        }
        for field in named_type.input_fields {
            validate_input_value(
                composer,
                &field,
                &format!("input field `{}.{}`", named_type.name, field.name.value),
            );
            if !composer.can_continue() {
                return;
            }
        }
    }
    validate_directive_arguments(composer);
}

fn validate_directive_arguments(composer: &mut Composer) {
    for index in 0..composer.schema.directives.len() {
        if !composer.can_continue() {
            return;
        }
        let definition = composer.schema.directives[index].clone();
        for argument in definition.arguments() {
            let Some(source) = &argument.source_definition else {
                continue;
            };
            validate_input_value(
                composer,
                source,
                &format!(
                    "directive argument `@{}.{}`",
                    definition.name(),
                    argument.name
                ),
            );
            if !composer.can_continue() {
                return;
            }
        }
    }
}

fn validate_input_value(composer: &mut Composer, input: &InputValueDefinition, coordinate: &str) {
    if !matches!(input.ty.kind, TypeRefKind::NonNull(_)) || input.default_value.is_some() {
        return;
    }
    for directive in named(&input.directives, "deprecated") {
        composer.report(
            super::super::SCHEMA_DEPRECATED_REQUIRED_INPUT,
            format!("Required {coordinate} cannot be deprecated"),
            directive.span,
        );
        if !composer.can_continue() {
            return;
        }
    }
}

fn named<'a>(directives: &'a [Directive], name: &'a str) -> impl Iterator<Item = &'a Directive> {
    directives
        .iter()
        .filter(move |directive| directive.name.value == name)
}
