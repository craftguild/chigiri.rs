//! GraphQL September 2025 § 4.2 introspection descriptors, kept separate from the user schema.

mod enums;
pub(crate) mod model;
mod objects;

pub(crate) use model::TypeDescriptor;

static TYPES: [TypeDescriptor; 8] = [
    objects::SCHEMA,
    objects::TYPE,
    objects::FIELD,
    objects::INPUT_VALUE,
    objects::ENUM_VALUE,
    objects::DIRECTIVE,
    enums::TYPE_KIND,
    enums::DIRECTIVE_LOCATION,
];

/// September 2025 § Return the 8 named type of 4.2 in the order specified.
pub(crate) const fn types() -> &'static [TypeDescriptor] {
    &TYPES
}

/// An introspection named type.
pub(crate) fn type_by_name(name: &str) -> Option<&'static TypeDescriptor> {
    TYPES.iter().find(|descriptor| descriptor.name == name)
}

/// An introspection meta-field available only on the query root.
pub(crate) fn root_field(name: &str) -> Option<&'static model::FieldDescriptor> {
    objects::root_field(name)
}
