//! Source-independent value types for introspection type descriptors.

/// Semantic model exposed through schema introspection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DescriptorKind {
    Object,
    Enum,
}

/// Modifier wrapping a named type; modifiers are stored from outermost to
/// innermost.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TypeWrapper {
    List,
    NonNull,
}

/// Flattened type reference that requires no runtime parsing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TypeRefDescriptor {
    pub(crate) named_type: &'static str,
    pub(crate) wrappers: &'static [TypeWrapper],
}

/// The static default value of the introspection field argument.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DefaultValueDescriptor {
    Boolean(bool),
}

/// Static descriptor for field arguments.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InputValueDescriptor {
    pub(crate) name: &'static str,
    pub(crate) description: Option<&'static str>,
    pub(crate) ty: TypeRefDescriptor,
    pub(crate) default_value: Option<DefaultValueDescriptor>,
}

/// The static descriptor of the object field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FieldDescriptor {
    pub(crate) name: &'static str,
    pub(crate) description: Option<&'static str>,
    pub(crate) ty: TypeRefDescriptor,
    pub(crate) arguments: &'static [InputValueDescriptor],
}

/// The static descriptor of enum value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EnumValueDescriptor {
    pub(crate) name: &'static str,
    pub(crate) description: Option<&'static str>,
}

/// One introspection named type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TypeDescriptor {
    pub(crate) name: &'static str,
    pub(crate) description: Option<&'static str>,
    pub(crate) kind: DescriptorKind,
    pub(crate) fields: &'static [FieldDescriptor],
    pub(crate) enum_values: &'static [EnumValueDescriptor],
}

pub(super) const NULLABLE: &[TypeWrapper] = &[];
pub(super) const NON_NULL: &[TypeWrapper] = &[TypeWrapper::NonNull];
pub(super) const LIST_OF_NON_NULL: &[TypeWrapper] = &[TypeWrapper::List, TypeWrapper::NonNull];
pub(super) const NON_NULL_LIST_OF_NON_NULL: &[TypeWrapper] = &[
    TypeWrapper::NonNull,
    TypeWrapper::List,
    TypeWrapper::NonNull,
];

pub(super) const fn field(
    name: &'static str,
    named_type: &'static str,
    wrappers: &'static [TypeWrapper],
    arguments: &'static [InputValueDescriptor],
) -> FieldDescriptor {
    FieldDescriptor {
        name,
        description: None,
        ty: TypeRefDescriptor {
            named_type,
            wrappers,
        },
        arguments,
    }
}

pub(super) const fn object(
    name: &'static str,
    fields: &'static [FieldDescriptor],
) -> TypeDescriptor {
    TypeDescriptor {
        name,
        description: None,
        kind: DescriptorKind::Object,
        fields,
        enum_values: &[],
    }
}

pub(super) const fn enumeration(
    name: &'static str,
    enum_values: &'static [EnumValueDescriptor],
) -> TypeDescriptor {
    TypeDescriptor {
        name,
        description: None,
        kind: DescriptorKind::Enum,
        fields: &[],
        enum_values,
    }
}
