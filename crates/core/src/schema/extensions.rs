//! Merge queued extensions in source order after collecting definitions.

use crate::language::ast::{
    EnumTypeExtension, InputObjectTypeExtension, InterfaceTypeExtension, Name, ObjectTypeExtension,
    ScalarTypeExtension, TypeSystemDefinition, UnionTypeExtension,
};
use std::collections::BTreeMap;

use super::composer::Composer;
use super::{SchemaOrigin, SemanticType, TypeKind};

pub(super) fn apply_extensions(composer: &mut Composer) {
    let extensions = std::mem::take(&mut composer.extensions);
    for extension in extensions {
        if !composer.can_continue() {
            return;
        }
        match extension {
            TypeSystemDefinition::SchemaExtension(value) => {
                super::roots::apply_schema_extension(composer, &value)
            }
            TypeSystemDefinition::ScalarExtension(value) => apply_scalar(composer, &value),
            TypeSystemDefinition::ObjectExtension(value) => apply_object(composer, &value),
            TypeSystemDefinition::InterfaceExtension(value) => apply_interface(composer, &value),
            TypeSystemDefinition::UnionExtension(value) => apply_union(composer, &value),
            TypeSystemDefinition::EnumExtension(value) => apply_enum(composer, &value),
            TypeSystemDefinition::InputObjectExtension(value) => apply_input(composer, &value),
            _ => unreachable!("The definition is applied in the collection phase."),
        }
    }
}

fn apply_scalar(composer: &mut Composer, extension: &ScalarTypeExtension) {
    let Some(index) = target_index(
        composer,
        &extension.name.value,
        TypeKind::Scalar,
        extension.name.span,
    ) else {
        return;
    };
    let target = &mut composer.schema.types[index];
    target.directives.extend(extension.directives.clone());
    target.directive_origins.extend(
        extension
            .directives
            .iter()
            .map(|_| SchemaOrigin::Extension(extension.span)),
    );
    target.origins.push(SchemaOrigin::Extension(extension.span));
}

fn apply_object(composer: &mut Composer, extension: &ObjectTypeExtension) {
    let Some(index) = target_index(
        composer,
        &extension.name.value,
        TypeKind::Object,
        extension.name.span,
    ) else {
        return;
    };
    super::members::check_fields(composer, &extension.fields);
    if !composer.can_continue() {
        return;
    }
    let duplicates = existing_duplicates(
        &composer.schema.types[index].field_index,
        extension.fields.iter().map(|field| &field.name),
    );
    report_existing_duplicates(composer, &duplicates, "field");
    if !composer.can_continue() {
        return;
    }
    let target = &mut composer.schema.types[index];
    target.append_implemented_interfaces(&extension.implements_interfaces);
    target.append_fields(&extension.fields);
    finish_extension(target, &extension.directives, extension.span);
}

fn apply_interface(composer: &mut Composer, extension: &InterfaceTypeExtension) {
    let Some(index) = target_index(
        composer,
        &extension.name.value,
        TypeKind::Interface,
        extension.name.span,
    ) else {
        return;
    };
    super::members::check_fields(composer, &extension.fields);
    if !composer.can_continue() {
        return;
    }
    let duplicates = existing_duplicates(
        &composer.schema.types[index].field_index,
        extension.fields.iter().map(|field| &field.name),
    );
    report_existing_duplicates(composer, &duplicates, "field");
    if !composer.can_continue() {
        return;
    }
    let target = &mut composer.schema.types[index];
    target.append_implemented_interfaces(&extension.implements_interfaces);
    target.append_fields(&extension.fields);
    finish_extension(target, &extension.directives, extension.span);
}

fn apply_union(composer: &mut Composer, extension: &UnionTypeExtension) {
    let Some(index) = target_index(
        composer,
        &extension.name.value,
        TypeKind::Union,
        extension.name.span,
    ) else {
        return;
    };
    let target = &mut composer.schema.types[index];
    target.append_union_members(&extension.members);
    finish_extension(target, &extension.directives, extension.span);
}

fn apply_enum(composer: &mut Composer, extension: &EnumTypeExtension) {
    let Some(index) = target_index(
        composer,
        &extension.name.value,
        TypeKind::Enum,
        extension.name.span,
    ) else {
        return;
    };
    super::members::check_enum_values(composer, &extension.values);
    if !composer.can_continue() {
        return;
    }
    let duplicates = existing_duplicates(
        &composer.schema.types[index].enum_value_index,
        extension.values.iter().map(|value| &value.name),
    );
    report_existing_duplicates(composer, &duplicates, "enum value");
    if !composer.can_continue() {
        return;
    }
    let target = &mut composer.schema.types[index];
    target.append_enum_values(&extension.values);
    finish_extension(target, &extension.directives, extension.span);
}

fn apply_input(composer: &mut Composer, extension: &InputObjectTypeExtension) {
    let Some(index) = target_index(
        composer,
        &extension.name.value,
        TypeKind::InputObject,
        extension.name.span,
    ) else {
        return;
    };
    super::members::check_input_values(composer, &extension.fields, "input field");
    if !composer.can_continue() {
        return;
    }
    let duplicates = existing_duplicates(
        &composer.schema.types[index].input_field_index,
        extension.fields.iter().map(|field| &field.name),
    );
    report_existing_duplicates(composer, &duplicates, "input field");
    if !composer.can_continue() {
        return;
    }
    let target = &mut composer.schema.types[index];
    target.append_input_fields(&extension.fields);
    finish_extension(target, &extension.directives, extension.span);
}

fn target_index(
    composer: &mut Composer,
    name: &str,
    expected_kind: TypeKind,
    span: crate::Span,
) -> Option<usize> {
    let Some(index) = composer.schema.type_index.get(name).copied() else {
        composer.report(
            super::SCHEMA_EXTENSION_TARGET_MISSING,
            format!("Cannot extend undefined type `{name}`"),
            span,
        );
        return None;
    };
    if composer.schema.types[index].kind != expected_kind {
        composer.report(
            super::SCHEMA_EXTENSION_KIND_MISMATCH,
            format!("Extension kind does not match type `{name}`"),
            span,
        );
        return None;
    }
    Some(index)
}

fn existing_duplicates<'a>(
    index: &BTreeMap<String, usize>,
    additions: impl IntoIterator<Item = &'a Name>,
) -> Vec<Name> {
    additions
        .into_iter()
        .filter(|name| index.contains_key(&name.value))
        .cloned()
        .collect()
}

fn report_existing_duplicates(composer: &mut Composer, names: &[Name], member_kind: &str) {
    for name in names {
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

fn finish_extension(
    target: &mut SemanticType,
    directives: &[crate::language::ast::Directive],
    span: crate::Span,
) {
    target.directives.extend(directives.to_vec());
    target
        .directive_origins
        .extend(directives.iter().map(|_| SchemaOrigin::Extension(span)));
    target.origins.push(SchemaOrigin::Extension(span));
}
