//! Compare equivalent vocabulary definitions without conflating locations, descriptions, or declaration order.

use chigiri::core::language::ast::{
    InputValueDefinition, TypeRef, TypeRefKind, TypeSystemDefinition as Def, Value,
};

pub(super) fn same(left: &Def, right: &Def) -> bool {
    match (left, right) {
        (Def::Directive(a), Def::Directive(b)) => {
            a.is_repeatable() == b.is_repeatable()
                && a.locations.len() == b.locations.len()
                && a.locations.iter().all(|location| {
                    b.locations
                        .iter()
                        .any(|other| location.value == other.value)
                })
                && inputs(&a.arguments, &b.arguments)
        }
        (Def::Enum(a), Def::Enum(b)) => {
            a.directives.is_empty()
                && b.directives.is_empty()
                && a.values.len() == b.values.len()
                && a.values.iter().all(|value| {
                    value.directives.is_empty()
                        && b.values.iter().any(|other| {
                            value.name.value == other.name.value && other.directives.is_empty()
                        })
                })
        }
        (Def::InputObject(a), Def::InputObject(b)) => {
            a.directives.is_empty() && b.directives.is_empty() && inputs(&a.fields, &b.fields)
        }
        _ => false,
    }
}

fn inputs(left: &[InputValueDefinition], right: &[InputValueDefinition]) -> bool {
    left.len() == right.len()
        && left.iter().all(|a| {
            right.iter().any(|b| {
                a.name.value == b.name.value
                    && a.directives.is_empty()
                    && b.directives.is_empty()
                    && type_ref(&a.ty, &b.ty)
                    && match (&a.default_value, &b.default_value) {
                        (None, None) => true,
                        (Some(a), Some(b)) => matches!((&a.value, &b.value),
                    (Value::String(a), Value::String(b)) if a.value == b.value),
                        _ => false,
                    }
            })
        })
}

fn type_ref(mut left: &TypeRef, mut right: &TypeRef) -> bool {
    loop {
        match (&left.kind, &right.kind) {
            (TypeRefKind::Named(a), TypeRefKind::Named(b)) => return a.value == b.value,
            (TypeRefKind::List(a), TypeRefKind::List(b))
            | (TypeRefKind::NonNull(a), TypeRefKind::NonNull(b)) => {
                left = a;
                right = b;
            }
            _ => return false,
        }
    }
}
