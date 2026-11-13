//! Rejects named types that remain empty after extensions are applied.

use super::super::composer::Composer;
use super::super::{SchemaOrigin, TypeKind};

pub(super) fn validate(composer: &mut Composer) {
    for index in 0..composer.schema.types.len() {
        if !composer.can_continue() {
            return;
        }
        let named_type = &composer.schema.types[index];
        let empty = match named_type.kind {
            TypeKind::Object | TypeKind::Interface => named_type.fields.is_empty(),
            TypeKind::Union => named_type.union_members.is_empty(),
            TypeKind::Enum => named_type.enum_values.is_empty(),
            TypeKind::InputObject => named_type.input_fields.is_empty(),
            TypeKind::Scalar => false,
        };
        if !empty {
            continue;
        }
        let name = named_type.name.clone();
        let kind = named_type.kind;
        let origin = named_type
            .origins
            .iter()
            .copied()
            .find(|origin| !matches!(origin, SchemaOrigin::BuiltIn | SchemaOrigin::Implicit))
            .unwrap_or(SchemaOrigin::BuiltIn);
        composer.report_origin(
            super::super::SCHEMA_EMPTY_MEMBERS,
            format!("{kind:?} type `{name}` must define at least one member"),
            origin,
        );
    }
}
