//! Validate interface fields, arguments, covariance, and deprecation after composition.

use crate::language::ast::{Directive, FieldDefinition, InputValueDefinition, TypeRefKind};
use std::collections::{BTreeMap, BTreeSet};

use super::super::composer::Composer;
use super::super::{SemanticType, TypeKind};

pub(super) fn validate(composer: &mut Composer) {
    for index in 0..composer.schema.types.len() {
        if !composer.can_continue() {
            return;
        }
        let implementor = composer.schema.types[index].clone();
        if !matches!(implementor.kind(), TypeKind::Object | TypeKind::Interface) {
            continue;
        }
        validate_implementor(composer, &implementor);
    }
}

fn validate_implementor(composer: &mut Composer, implementor: &SemanticType) {
    let mut seen = BTreeSet::new();
    for implemented in implementor.implemented_interfaces() {
        if !seen.insert(implemented.value.as_str()) || implemented.value == implementor.name() {
            continue;
        }
        let Some(interface) = composer.schema.type_by_name(&implemented.value).cloned() else {
            continue;
        };
        if interface.kind() != TypeKind::Interface {
            continue;
        }
        validate_pair(composer, implementor, &interface, implemented.span);
        if !composer.can_continue() {
            return;
        }
    }
}

fn validate_pair(
    composer: &mut Composer,
    implementor: &SemanticType,
    interface: &SemanticType,
    implementation_span: crate::Span,
) {
    for interface_field in interface.fields() {
        let Some(field) = implementor.field(&interface_field.name.value) else {
            composer.report(
                super::super::SCHEMA_INTERFACE_FIELD_MISSING,
                format!(
                    "Type `{}` must define interface field `{}.{}`",
                    implementor.name(),
                    interface.name(),
                    interface_field.name.value
                ),
                implementation_span,
            );
            if !composer.can_continue() {
                return;
            }
            continue;
        };
        validate_arguments(composer, implementor, interface_field, field);
        if !composer.can_continue() {
            return;
        }
        validate_return_type(composer, implementor, interface_field, field);
        if !composer.can_continue() {
            return;
        }
        validate_deprecation(composer, implementor, interface_field, field);
        if !composer.can_continue() {
            return;
        }
    }
}

fn validate_arguments(
    composer: &mut Composer,
    implementor: &SemanticType,
    interface_field: &FieldDefinition,
    field: &FieldDefinition,
) {
    let implementation_arguments = argument_index(field);
    let interface_arguments = argument_index(interface_field);
    for interface_argument in &interface_field.arguments {
        let Some(argument) = implementation_arguments.get(interface_argument.name.value.as_str())
        else {
            composer.report(
                super::super::SCHEMA_INTERFACE_ARGUMENT_MISSING,
                format!(
                    "Field `{}.{}` must define interface argument `{}`",
                    implementor.name(),
                    field.name.value,
                    interface_argument.name.value
                ),
                interface_argument.name.span,
            );
            if !composer.can_continue() {
                return;
            }
            continue;
        };
        if super::interfaces_covariance::both_resolved(
            &composer.schema,
            &argument.ty,
            &interface_argument.ty,
        ) && !super::interfaces_covariance::same_type(&argument.ty, &interface_argument.ty)
        {
            composer.report(
                super::super::SCHEMA_INTERFACE_ARGUMENT_TYPE_MISMATCH,
                format!(
                    "Argument `{}.{}` must use the exact interface argument type",
                    implementor.name(),
                    argument.name.value
                ),
                argument.ty.span,
            );
            if !composer.can_continue() {
                return;
            }
        }
    }
    for extra in &field.arguments {
        if !interface_arguments.contains_key(extra.name.value.as_str()) && is_required(extra) {
            composer.report(
                super::super::SCHEMA_INTERFACE_EXTRA_REQUIRED_ARGUMENT,
                format!(
                    "Field `{}.{}` cannot add required argument `{}`",
                    implementor.name(),
                    field.name.value,
                    extra.name.value
                ),
                extra.name.span,
            );
            if !composer.can_continue() {
                return;
            }
        }
    }
}

fn validate_return_type(
    composer: &mut Composer,
    implementor: &SemanticType,
    interface_field: &FieldDefinition,
    field: &FieldDefinition,
) {
    if !super::interfaces_covariance::both_resolved(
        &composer.schema,
        &field.ty,
        &interface_field.ty,
    ) || super::interfaces_covariance::is_valid_implementation_type(
        &composer.schema,
        &field.ty,
        &interface_field.ty,
    ) {
        return;
    }
    composer.report(
        super::super::SCHEMA_INTERFACE_RETURN_TYPE_MISMATCH,
        format!(
            "Field `{}.{}` must return a subtype of its interface field",
            implementor.name(),
            field.name.value
        ),
        field.ty.span,
    );
}

fn validate_deprecation(
    composer: &mut Composer,
    implementor: &SemanticType,
    interface_field: &FieldDefinition,
    field: &FieldDefinition,
) {
    let Some(deprecated) = named(&field.directives, "deprecated") else {
        return;
    };
    if named(&interface_field.directives, "deprecated").is_some() {
        return;
    }
    composer.report(
        super::super::SCHEMA_INTERFACE_DEPRECATION_MISMATCH,
        format!(
            "Deprecated field `{}.{}` requires the interface field to be deprecated",
            implementor.name(),
            field.name.value
        ),
        deprecated.span,
    );
}

fn argument_index(field: &FieldDefinition) -> BTreeMap<&str, &InputValueDefinition> {
    field
        .arguments
        .iter()
        .map(|argument| (argument.name.value.as_str(), argument))
        .collect()
}

fn is_required(argument: &InputValueDefinition) -> bool {
    matches!(argument.ty.kind, TypeRefKind::NonNull(_)) && argument.default_value.is_none()
}

fn named<'a>(directives: &'a [Directive], name: &str) -> Option<&'a Directive> {
    directives
        .iter()
        .find(|directive| directive.name.value == name)
}
