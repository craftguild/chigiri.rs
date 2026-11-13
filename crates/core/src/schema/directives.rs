//! Validate directive applications at every type-system coordinate after composition.

use std::collections::BTreeSet;

use crate::language::ast::{Directive, DirectiveLocation, Value, ValueNode};

use super::TypeKind;
use super::composer::Composer;

pub(super) fn validate_directive_applications(composer: &mut Composer) {
    validate_schema(composer);
    for type_index in 0..composer.schema.types.len() {
        if !composer.can_continue() {
            return;
        }
        validate_type(composer, type_index);
    }
    for directive_index in 0..composer.schema.directives.len() {
        if !composer.can_continue() {
            return;
        }
        validate_directive_definition(composer, directive_index);
    }
}

fn validate_schema(composer: &mut Composer) {
    let directives = composer.schema.schema_directives.clone();
    validate_coordinate(composer, &directives, DirectiveLocation::Schema);
}

fn validate_type(composer: &mut Composer, type_index: usize) {
    let named_type = composer.schema.types[type_index].clone();
    validate_coordinate(
        composer,
        &named_type.directives,
        type_location(named_type.kind),
    );
    if !composer.can_continue() {
        return;
    }
    for field in named_type.fields {
        validate_coordinate(
            composer,
            &field.directives,
            DirectiveLocation::FieldDefinition,
        );
        for argument in field.arguments {
            if !composer.can_continue() {
                return;
            }
            validate_coordinate(
                composer,
                &argument.directives,
                DirectiveLocation::ArgumentDefinition,
            );
        }
        if !composer.can_continue() {
            return;
        }
    }
    for value in named_type.enum_values {
        validate_coordinate(composer, &value.directives, DirectiveLocation::EnumValue);
        if !composer.can_continue() {
            return;
        }
    }
    for field in named_type.input_fields {
        validate_coordinate(
            composer,
            &field.directives,
            DirectiveLocation::InputFieldDefinition,
        );
        if !composer.can_continue() {
            return;
        }
    }
}

fn type_location(kind: TypeKind) -> DirectiveLocation {
    match kind {
        TypeKind::Scalar => DirectiveLocation::Scalar,
        TypeKind::Object => DirectiveLocation::Object,
        TypeKind::Interface => DirectiveLocation::Interface,
        TypeKind::Union => DirectiveLocation::Union,
        TypeKind::Enum => DirectiveLocation::Enum,
        TypeKind::InputObject => DirectiveLocation::InputObject,
    }
}

fn validate_directive_definition(composer: &mut Composer, directive_index: usize) {
    let definition = composer.schema.directives[directive_index].clone();
    if definition.locations().is_empty() {
        composer.report_origin(
            super::SCHEMA_DIRECTIVE_LOCATION_MISSING,
            format!(
                "Directive definition `@{}` must include at least one location",
                definition.name()
            ),
            definition.origin(),
        );
        if !composer.can_continue() {
            return;
        }
    }
    let arguments = definition.arguments().to_vec();
    for argument in arguments {
        if let Some(source) = argument.source_definition {
            validate_coordinate(
                composer,
                &source.directives,
                DirectiveLocation::ArgumentDefinition,
            );
        }
        if !composer.can_continue() {
            return;
        }
    }
}

fn validate_coordinate(
    composer: &mut Composer,
    directives: &[Directive],
    location: DirectiveLocation,
) {
    let mut seen = BTreeSet::new();
    for directive in directives {
        let definition_index = composer
            .schema
            .directive_index
            .get(&directive.name.value)
            .copied();
        match definition_index {
            Some(index) => {
                validate_allowed_location(composer, directive, index, location);
                if !composer.can_continue() {
                    return;
                }
                if !composer.schema.directives[index].is_repeatable()
                    && !seen.insert(directive.name.value.as_str())
                {
                    composer.report(
                        super::SCHEMA_DUPLICATE_DIRECTIVE_APPLICATION,
                        format!(
                            "Non-repeatable directive `@{}` is applied more than once",
                            directive.name.value
                        ),
                        directive.span,
                    );
                }
            }
            None => composer.report(
                super::SCHEMA_UNKNOWN_DIRECTIVE,
                format!("Directive `@{}` is not defined", directive.name.value),
                directive.name.span,
            ),
        }
        if !composer.can_continue() {
            return;
        }
        validate_arguments(composer, directive, definition_index);
        if !composer.can_continue() {
            return;
        }
    }
}

fn validate_allowed_location(
    composer: &mut Composer,
    directive: &Directive,
    definition_index: usize,
    location: DirectiveLocation,
) {
    if has_specialized_target_validation(directive, location) {
        return;
    }
    if composer.schema.directives[definition_index].allows_location(location) {
        return;
    }
    composer.report(
        super::SCHEMA_INVALID_DIRECTIVE_LOCATION,
        format!(
            "Directive `@{}` cannot be used at {location:?}",
            directive.name.value
        ),
        directive.name.span,
    );
}

fn has_specialized_target_validation(directive: &Directive, location: DirectiveLocation) -> bool {
    let named_type_location = matches!(
        location,
        DirectiveLocation::Scalar
            | DirectiveLocation::Object
            | DirectiveLocation::Interface
            | DirectiveLocation::Union
            | DirectiveLocation::Enum
            | DirectiveLocation::InputObject
    );
    named_type_location && matches!(directive.name.value.as_str(), "oneOf" | "specifiedBy")
}

fn validate_arguments(
    composer: &mut Composer,
    directive: &Directive,
    definition_index: Option<usize>,
) {
    let mut seen = BTreeSet::new();
    for argument in &directive.arguments {
        if !seen.insert(argument.name.value.as_str()) {
            composer.report(
                super::SCHEMA_DUPLICATE_DIRECTIVE_ARGUMENT,
                format!(
                    "Directive `@{}` provides argument `{}` more than once",
                    directive.name.value, argument.name.value
                ),
                argument.name.span,
            );
        }
        if !composer.can_continue() {
            return;
        }
        let unknown = definition_index.is_some_and(|index| {
            composer.schema.directives[index]
                .argument(&argument.name.value)
                .is_none()
        });
        if unknown {
            composer.report(
                super::SCHEMA_UNKNOWN_DIRECTIVE_ARGUMENT,
                format!(
                    "Directive `@{}` does not define argument `{}`",
                    directive.name.value, argument.name.value
                ),
                argument.name.span,
            );
        }
        if !composer.can_continue() {
            return;
        }
        reject_variables(composer, &directive.name.value, &argument.value);
        if !composer.can_continue() {
            return;
        }
    }
    if let Some(index) = definition_index {
        reject_missing_required(composer, directive, index, &seen);
    }
}

fn reject_missing_required(
    composer: &mut Composer,
    directive: &Directive,
    definition_index: usize,
    provided: &BTreeSet<&str>,
) {
    let required_count = composer.schema.directives[definition_index]
        .required_argument_indices
        .len();
    for position in 0..required_count {
        let missing_name = {
            let definition = &composer.schema.directives[definition_index];
            let argument_index = definition.required_argument_indices[position];
            let name = &definition.arguments[argument_index].name;
            (!provided.contains(name.as_str())).then(|| name.clone())
        };
        if let Some(name) = missing_name {
            composer.report(
                super::SCHEMA_MISSING_DIRECTIVE_ARGUMENT,
                format!(
                    "Directive `@{}` requires argument `{}`",
                    directive.name.value, name
                ),
                directive.span,
            );
            if !composer.can_continue() {
                return;
            }
        }
    }
}

fn reject_variables(composer: &mut Composer, directive_name: &str, value: &ValueNode) {
    let mut work = vec![value];
    while let Some(current) = work.pop() {
        match &current.value {
            Value::Variable(name) => composer.report(
                super::SCHEMA_DIRECTIVE_ARGUMENT_VARIABLE,
                format!(
                    "Directive `@{directive_name}` type-system argument cannot contain variable `${}`",
                    name.value
                ),
                current.span,
            ),
            Value::List(items) => work.extend(items.iter().rev()),
            Value::Object(fields) => {
                work.extend(fields.iter().rev().map(|field| &field.value));
            }
            Value::Int(_)
            | Value::Float(_)
            | Value::String(_)
            | Value::Boolean(_)
            | Value::Null
            | Value::Enum(_) => {}
        }
        if !composer.can_continue() {
            return;
        }
    }
}
