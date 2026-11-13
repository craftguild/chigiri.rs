//! Validates interface-field return covariance through wrappers and named subtypes.

use crate::language::ast::{TypeRef, TypeRefKind};

use super::super::{SemanticSchema, TypeKind};

pub(super) fn same_type(left: &TypeRef, right: &TypeRef) -> bool {
    match (&left.kind, &right.kind) {
        (TypeRefKind::Named(left), TypeRefKind::Named(right)) => left.value == right.value,
        (TypeRefKind::List(left), TypeRefKind::List(right))
        | (TypeRefKind::NonNull(left), TypeRefKind::NonNull(right)) => same_type(left, right),
        _ => false,
    }
}

pub(super) fn both_resolved(schema: &SemanticSchema, left: &TypeRef, right: &TypeRef) -> bool {
    schema.type_by_name(base_name(left)).is_some()
        && schema.type_by_name(base_name(right)).is_some()
}

pub(super) fn is_valid_implementation_type(
    schema: &SemanticSchema,
    implementation: &TypeRef,
    interface: &TypeRef,
) -> bool {
    match &implementation.kind {
        TypeRefKind::NonNull(implementation_inner) => {
            let interface_inner = match &interface.kind {
                TypeRefKind::NonNull(interface_inner) => interface_inner.as_ref(),
                _ => interface,
            };
            is_valid_implementation_type(schema, implementation_inner, interface_inner)
        }
        TypeRefKind::List(implementation_item) => {
            let TypeRefKind::List(interface_item) = &interface.kind else {
                return false;
            };
            is_valid_implementation_type(schema, implementation_item, interface_item)
        }
        TypeRefKind::Named(implementation_name) => {
            let TypeRefKind::Named(interface_name) = &interface.kind else {
                return false;
            };
            is_named_subtype(schema, &implementation_name.value, &interface_name.value)
        }
    }
}

fn base_name(reference: &TypeRef) -> &str {
    match &reference.kind {
        TypeRefKind::Named(name) => &name.value,
        TypeRefKind::List(inner) | TypeRefKind::NonNull(inner) => base_name(inner),
    }
}

fn is_named_subtype(schema: &SemanticSchema, implementation: &str, interface: &str) -> bool {
    if implementation == interface {
        return true;
    }
    let (Some(implementation_type), Some(interface_type)) = (
        schema.type_by_name(implementation),
        schema.type_by_name(interface),
    ) else {
        return false;
    };
    if implementation_type.kind() == TypeKind::Object && interface_type.kind() == TypeKind::Union {
        return interface_type.has_union_member(implementation);
    }
    matches!(
        (implementation_type.kind(), interface_type.kind()),
        (TypeKind::Object | TypeKind::Interface, TypeKind::Interface)
    ) && implementation_type.implements_interface(interface)
}
