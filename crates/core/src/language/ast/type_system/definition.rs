//! Type-definition syntax from scalars through input objects.

use super::super::{Description, Directive, Name};
use super::{EnumValueDefinition, FieldDefinition, InputValueDefinition};
use crate::Span;

/// scalar type definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScalarTypeDefinition {
    /// Optional description published through introspection.
    pub description: Option<Description>,
    /// Scalar name.
    pub name: Name,
    /// type-system directive application.
    pub directives: Vec<Directive>,
    /// Source span of the complete definition.
    pub span: Span,
}

/// object type definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectTypeDefinition {
    /// Optional description published through introspection.
    pub description: Option<Description>,
    /// Object type name.
    pub name: Name,
    /// Implemented-interface names in source order.
    pub implements_interfaces: Vec<Name>,
    /// type-system directive application.
    pub directives: Vec<Directive>,
    /// Field definitions in source order.
    pub fields: Vec<FieldDefinition>,
    /// Source span of the complete definition.
    pub span: Span,
}

/// interface type definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterfaceTypeDefinition {
    /// Optional description published through introspection.
    pub description: Option<Description>,
    /// Interface name.
    pub name: Name,
    /// Implemented-interface names in source order.
    pub implements_interfaces: Vec<Name>,
    /// type-system directive application.
    pub directives: Vec<Directive>,
    /// Field definitions in source order.
    pub fields: Vec<FieldDefinition>,
    /// Source span of the complete definition.
    pub span: Span,
}

/// union type definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnionTypeDefinition {
    /// Optional description published through introspection.
    pub description: Option<Description>,
    /// Union name.
    pub name: Name,
    /// Member object-type names in source order.
    pub members: Vec<Name>,
    /// type-system directive application.
    pub directives: Vec<Directive>,
    /// Source span of the complete definition.
    pub span: Span,
}

/// enum type definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnumTypeDefinition {
    /// Optional description published through introspection.
    pub description: Option<Description>,
    /// Enum name.
    pub name: Name,
    /// type-system directive application.
    pub directives: Vec<Directive>,
    /// Enum-value definitions in source order.
    pub values: Vec<EnumValueDefinition>,
    /// Source span of the complete definition.
    pub span: Span,
}

/// input object type definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputObjectTypeDefinition {
    /// Optional description published through introspection.
    pub description: Option<Description>,
    /// Input-object name.
    pub name: Name,
    /// type-system directive application, including `@oneOf`.
    pub directives: Vec<Directive>,
    /// Input-field definitions in source order.
    pub fields: Vec<InputValueDefinition>,
    /// Source span of the complete definition.
    pub span: Span,
}

#[cfg(test)]
mod tests {
    use super::{ObjectTypeDefinition, UnionTypeDefinition};
    use crate::language::ast::Name;
    use crate::{SourceId, Span};

    fn span(start: usize, end: usize) -> Span {
        Span::new(SourceId::new(7), start, end)
    }

    #[test]
    fn objects_preserve_interface_source_order() {
        let object = ObjectTypeDefinition {
            description: None,
            name: Name::new("Image".to_owned(), span(5, 10)),
            implements_interfaces: vec![
                Name::new("Node".to_owned(), span(22, 26)),
                Name::new("Resource".to_owned(), span(29, 37)),
            ],
            directives: Vec::new(),
            fields: Vec::new(),
            span: span(0, 40),
        };

        assert_eq!(object.implements_interfaces[0].value, "Node");
        assert_eq!(object.implements_interfaces[1].value, "Resource");
    }

    #[test]
    fn unions_preserve_each_member_origin() {
        let union = UnionTypeDefinition {
            description: None,
            name: Name::new("Result".to_owned(), span(6, 12)),
            members: vec![
                Name::new("Photo".to_owned(), span(15, 20)),
                Name::new("Person".to_owned(), span(23, 29)),
            ],
            directives: Vec::new(),
            span: span(0, 29),
        };

        assert_ne!(union.members[0].span, union.members[1].span);
    }
}
