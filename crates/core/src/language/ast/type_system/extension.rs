//! Preserves kind-specific type-extension syntax before schema composition.

use super::super::{Directive, Name};
use super::{EnumValueDefinition, FieldDefinition, InputValueDefinition};
use crate::Span;

/// scalar type extension.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScalarTypeExtension {
    /// Name of the scalar being extended.
    pub name: Name,
    /// Directive applications added by the extension.
    pub directives: Vec<Directive>,
    /// Source span of the complete extension.
    pub span: Span,
}

/// object type extension.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectTypeExtension {
    /// Name of the object type being extended.
    pub name: Name,
    /// Implemented interfaces added by the extension.
    pub implements_interfaces: Vec<Name>,
    /// Directive applications added by the extension.
    pub directives: Vec<Directive>,
    /// Field definitions added by the extension.
    pub fields: Vec<FieldDefinition>,
    /// Source span of the complete extension.
    pub span: Span,
}

/// interface type extension.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterfaceTypeExtension {
    /// Name of the interface being extended.
    pub name: Name,
    /// Implemented interfaces added by the extension.
    pub implements_interfaces: Vec<Name>,
    /// Directive applications added by the extension.
    pub directives: Vec<Directive>,
    /// Field definitions added by the extension.
    pub fields: Vec<FieldDefinition>,
    /// Source span of the complete extension.
    pub span: Span,
}

/// union type extension.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnionTypeExtension {
    /// Name of the union being extended.
    pub name: Name,
    /// Member object-type names added by the extension.
    pub members: Vec<Name>,
    /// Directive applications added by the extension.
    pub directives: Vec<Directive>,
    /// Source span of the complete extension.
    pub span: Span,
}

/// enum type extension.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnumTypeExtension {
    /// Name of the enum being extended.
    pub name: Name,
    /// Directive applications added by the extension.
    pub directives: Vec<Directive>,
    /// Enum-value definitions added by the extension.
    pub values: Vec<EnumValueDefinition>,
    /// Source span of the complete extension.
    pub span: Span,
}

/// input object type extension.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputObjectTypeExtension {
    /// Name of the input object being extended.
    pub name: Name,
    /// Directive applications added by the extension.
    pub directives: Vec<Directive>,
    /// Input-field definitions added by the extension.
    pub fields: Vec<InputValueDefinition>,
    /// Source span of the complete extension.
    pub span: Span,
}

#[cfg(test)]
mod tests {
    use super::{ObjectTypeExtension, ScalarTypeExtension};
    use crate::language::ast::Name;
    use crate::{SourceId, Span};

    fn span(start: usize, end: usize) -> Span {
        Span::new(SourceId::new(8), start, end)
    }

    #[test]
    fn extension_uses_a_distinct_syntax_type_from_definition() {
        let scalar = ScalarTypeExtension {
            name: Name::new("Date".to_owned(), span(14, 18)),
            directives: Vec::new(),
            span: span(0, 18),
        };
        let object = ObjectTypeExtension {
            name: Name::new("User".to_owned(), span(14, 18)),
            implements_interfaces: Vec::new(),
            directives: Vec::new(),
            fields: Vec::new(),
            span: span(0, 21),
        };

        assert_eq!(scalar.name.value, "Date");
        assert_eq!(object.name.value, "User");
    }
}
