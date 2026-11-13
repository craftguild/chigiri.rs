//! Shared duplicate-member rules for definitions and extensions.

use crate::language::ast::{
    DirectiveDefinition, EnumValueDefinition, FieldDefinition, InputValueDefinition, Name,
};
use std::collections::BTreeSet;

use super::composer::Composer;

pub(super) fn check_names<'a>(
    composer: &mut Composer,
    names: impl IntoIterator<Item = &'a Name>,
    member_kind: &str,
) {
    let mut seen = BTreeSet::new();
    for name in names {
        if !seen.insert(name.value.as_str()) {
            composer.report(
                super::SCHEMA_DUPLICATE_MEMBER,
                format!("Duplicate {member_kind} `{}`", name.value),
                name.span,
            );
            if !composer.can_continue() {
                return;
            }
        }
    }
}

pub(super) fn check_fields(composer: &mut Composer, fields: &[FieldDefinition]) {
    check_names(composer, fields.iter().map(|field| &field.name), "field");
    for field in fields {
        if !composer.can_continue() {
            return;
        }
        check_input_values(composer, &field.arguments, "argument");
    }
}

pub(super) fn check_input_values(
    composer: &mut Composer,
    values: &[InputValueDefinition],
    member_kind: &str,
) {
    check_names(
        composer,
        values.iter().map(|value| &value.name),
        member_kind,
    );
}

pub(super) fn check_enum_values(composer: &mut Composer, values: &[EnumValueDefinition]) {
    check_names(
        composer,
        values.iter().map(|value| &value.name),
        "enum value",
    );
}

pub(super) fn check_directive_definition(
    composer: &mut Composer,
    definition: &DirectiveDefinition,
) -> bool {
    let diagnostics_before = composer.diagnostics.len();
    check_input_values(composer, &definition.arguments, "directive argument");
    if !composer.can_continue() {
        return false;
    }
    if definition.locations.is_empty() {
        composer.report(
            super::SCHEMA_DIRECTIVE_LOCATION_MISSING,
            format!(
                "Directive definition `@{}` must include at least one location",
                definition.name.value
            ),
            definition.name.span,
        );
    }
    let mut seen = BTreeSet::new();
    for location in &definition.locations {
        if !seen.insert(location.value as u32) {
            composer.report(
                super::SCHEMA_DUPLICATE_MEMBER,
                format!("Duplicate directive location `{:?}`", location.value),
                location.span,
            );
            if !composer.can_continue() {
                return false;
            }
        }
    }
    composer.diagnostics.len() == diagnostics_before
}
