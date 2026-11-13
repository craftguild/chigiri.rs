//! Introspection enum values defined by the GraphQL September 2025 specification.

use super::model::{EnumValueDescriptor, TypeDescriptor, enumeration};

const fn value(name: &'static str) -> EnumValueDescriptor {
    EnumValueDescriptor {
        name,
        description: None,
    }
}

const TYPE_KIND_VALUES: &[EnumValueDescriptor] = &[
    value("SCALAR"),
    value("OBJECT"),
    value("INTERFACE"),
    value("UNION"),
    value("ENUM"),
    value("INPUT_OBJECT"),
    value("LIST"),
    value("NON_NULL"),
];

const DIRECTIVE_LOCATION_VALUES: &[EnumValueDescriptor] = &[
    value("QUERY"),
    value("MUTATION"),
    value("SUBSCRIPTION"),
    value("FIELD"),
    value("FRAGMENT_DEFINITION"),
    value("FRAGMENT_SPREAD"),
    value("INLINE_FRAGMENT"),
    value("VARIABLE_DEFINITION"),
    value("SCHEMA"),
    value("SCALAR"),
    value("OBJECT"),
    value("FIELD_DEFINITION"),
    value("ARGUMENT_DEFINITION"),
    value("INTERFACE"),
    value("UNION"),
    value("ENUM"),
    value("ENUM_VALUE"),
    value("INPUT_OBJECT"),
    value("INPUT_FIELD_DEFINITION"),
];

pub(super) const TYPE_KIND: TypeDescriptor = enumeration("__TypeKind", TYPE_KIND_VALUES);
pub(super) const DIRECTIVE_LOCATION: TypeDescriptor =
    enumeration("__DirectiveLocation", DIRECTIVE_LOCATION_VALUES);
