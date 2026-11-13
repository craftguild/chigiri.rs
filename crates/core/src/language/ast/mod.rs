//! Shared syntax tree for GraphQL executable documents and SDL token streams.
//!
//! This layer losslessly preserves source order, duplicates, and lexemes;
//! semantic checks such as name uniqueness and type validity are deferred to
//! composition and validation.

mod common;
mod directive;
mod document;
mod executable;
mod type_system;
mod value;

pub use common::{Name, OperationType, Spanned, TypeRef, TypeRefKind};
pub use directive::{Argument, Directive, DirectiveApplication};
pub use document::{Definition, Document};
pub use executable::{
    ExecutableDefinition, Field, FragmentDefinition, FragmentSpread, InlineFragment,
    OperationDefinition, Selection, SelectionSet, VariableDefinition,
};
pub use type_system::{
    DirectiveDefinition, DirectiveLocation, EnumTypeDefinition, EnumTypeExtension,
    EnumValueDefinition, FieldDefinition, InputObjectTypeDefinition, InputObjectTypeExtension,
    InputValueDefinition, InterfaceTypeDefinition, InterfaceTypeExtension, ObjectTypeDefinition,
    ObjectTypeExtension, RootOperationTypeDefinition, ScalarTypeDefinition, ScalarTypeExtension,
    SchemaDefinition, SchemaExtension, TypeSystemDefinition, UnionTypeDefinition,
    UnionTypeExtension,
};
pub use value::{Description, ObjectField, StringStyle, StringValue, Value, ValueNode};
