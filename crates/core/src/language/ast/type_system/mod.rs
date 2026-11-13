//! Schema definitions, type definitions, and extensions.

mod definition;
mod directive;
mod extension;
mod member;
mod schema;

pub use definition::{
    EnumTypeDefinition, InputObjectTypeDefinition, InterfaceTypeDefinition, ObjectTypeDefinition,
    ScalarTypeDefinition, UnionTypeDefinition,
};
pub use directive::{DirectiveDefinition, DirectiveLocation};
pub use extension::{
    EnumTypeExtension, InputObjectTypeExtension, InterfaceTypeExtension, ObjectTypeExtension,
    ScalarTypeExtension, UnionTypeExtension,
};
pub use member::{EnumValueDefinition, FieldDefinition, InputValueDefinition};
pub use schema::{RootOperationTypeDefinition, SchemaDefinition, SchemaExtension};

/// Definition or extension in a type-system document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeSystemDefinition {
    /// schema definition.
    Schema(SchemaDefinition),
    /// scalar type definition.
    Scalar(ScalarTypeDefinition),
    /// object type definition.
    Object(ObjectTypeDefinition),
    /// interface type definition.
    Interface(InterfaceTypeDefinition),
    /// union type definition.
    Union(UnionTypeDefinition),
    /// enum type definition.
    Enum(EnumTypeDefinition),
    /// input object type definition.
    InputObject(InputObjectTypeDefinition),
    /// directive definition.
    Directive(DirectiveDefinition),
    /// schema extension.
    SchemaExtension(SchemaExtension),
    /// scalar type extension.
    ScalarExtension(ScalarTypeExtension),
    /// object type extension.
    ObjectExtension(ObjectTypeExtension),
    /// interface type extension.
    InterfaceExtension(InterfaceTypeExtension),
    /// union type extension.
    UnionExtension(UnionTypeExtension),
    /// enum type extension.
    EnumExtension(EnumTypeExtension),
    /// input object type extension.
    InputObjectExtension(InputObjectTypeExtension),
}
