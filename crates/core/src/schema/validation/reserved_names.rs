//! Rejects the introspection-reserved prefix on user-defined schema elements.

use crate::Span;

use super::super::composer::Composer;

pub(super) fn validate(composer: &mut Composer) {
    for index in 0..composer.schema.types.len() {
        if !composer.can_continue() {
            return;
        }
        let named_type = composer.schema.types[index].clone();
        let user_defined = !named_type
            .origins
            .iter()
            .all(|origin| *origin == super::super::SchemaOrigin::BuiltIn);
        if user_defined && !check(composer, &named_type.name, named_type.name_span, "type") {
            return;
        }
        for field in named_type.fields {
            if !check(composer, &field.name.value, Some(field.name.span), "field") {
                return;
            }
            for argument in field.arguments {
                if !check(
                    composer,
                    &argument.name.value,
                    Some(argument.name.span),
                    "field argument",
                ) {
                    return;
                }
            }
        }
        for field in named_type.input_fields {
            if !check(
                composer,
                &field.name.value,
                Some(field.name.span),
                "input field",
            ) {
                return;
            }
        }
        for value in named_type.enum_values {
            if !check(
                composer,
                &value.name.value,
                Some(value.name.span),
                "enum value",
            ) {
                return;
            }
        }
    }
    validate_directives(composer);
}

fn validate_directives(composer: &mut Composer) {
    for index in 0..composer.schema.directives.len() {
        if !composer.can_continue() {
            return;
        }
        let definition = composer.schema.directives[index].clone();
        if definition.origin() == super::super::SchemaOrigin::BuiltIn {
            continue;
        }
        if !check(
            composer,
            definition.name(),
            definition
                .source_definition()
                .map(|source| source.name.span),
            "directive",
        ) {
            return;
        }
        for argument in definition.arguments() {
            if !check(
                composer,
                &argument.name,
                argument
                    .source_definition
                    .as_ref()
                    .map(|source| source.name.span),
                "directive argument",
            ) {
                return;
            }
        }
    }
}

fn check(composer: &mut Composer, name: &str, span: Option<Span>, element: &str) -> bool {
    if !name.starts_with("__") {
        return composer.can_continue();
    }
    let message = format!("User-defined {element} name `{name}` must not begin with `__`");
    if let Some(span) = span {
        composer.report(super::super::SCHEMA_RESERVED_NAME, message, span);
    } else {
        composer.report_without_span(super::super::SCHEMA_RESERVED_NAME, message);
    }
    composer.can_continue()
}
