//! Selection model used by introspection execution.

use crate::Value;
use crate::language::ast::{EnumValueDefinition, FieldDefinition, InputValueDefinition};
use crate::schema::introspection::model::{
    EnumValueDescriptor, FieldDescriptor, InputValueDescriptor, TypeDescriptor,
};
use crate::schema::{
    SemanticDirectiveArgument, SemanticDirectiveDefinition, SemanticType, SemanticTypeRef,
};

#[derive(Clone, Copy)]
pub(super) enum NamedType<'a> {
    Semantic(&'a SemanticType),
    Introspection(&'static TypeDescriptor),
}

impl<'a> NamedType<'a> {
    pub(super) fn name_ref(&self) -> &'a str {
        match self {
            Self::Semantic(ty) => ty.name(),
            Self::Introspection(ty) => ty.name,
        }
    }
}

#[derive(Clone)]
pub(super) enum TypeView<'a> {
    Named(NamedType<'a>),
    List(Box<TypeView<'a>>),
    NonNull(Box<TypeView<'a>>),
}

#[derive(Clone, Copy)]
pub(super) enum FieldView<'a> {
    Semantic(&'a FieldDefinition),
    Introspection(&'static FieldDescriptor),
}

#[derive(Clone, Copy)]
pub(super) enum InputValueView<'a> {
    Ast(&'a InputValueDefinition),
    Directive(&'a SemanticDirectiveArgument),
    Introspection(&'static InputValueDescriptor),
}

#[derive(Clone, Copy)]
pub(super) enum EnumValueView<'a> {
    Semantic(&'a EnumValueDefinition),
    Introspection(&'static EnumValueDescriptor),
}

#[derive(Clone)]
pub(super) enum Node<'a> {
    Schema,
    Type(TypeView<'a>),
    Field(FieldView<'a>),
    InputValue(InputValueView<'a>),
    EnumValue(EnumValueView<'a>),
    Directive(&'a SemanticDirectiveDefinition),
}

impl Node<'_> {
    pub(super) const fn runtime_type(&self) -> &'static str {
        match self {
            Self::Schema => "__Schema",
            Self::Type(_) => "__Type",
            Self::Field(_) => "__Field",
            Self::InputValue(_) => "__InputValue",
            Self::EnumValue(_) => "__EnumValue",
            Self::Directive(_) => "__Directive",
        }
    }
}

pub(super) enum Output<'a> {
    Null,
    Leaf(Value),
    Text(&'a str),
    Default(&'a crate::language::ast::Value),
    Node(Node<'a>),
    List(Vec<Output<'a>>),
}

impl<'a> Output<'a> {
    pub(super) fn optional_node(node: Option<Node<'a>>) -> Self {
        node.map_or(Self::Null, Self::Node)
    }
}

pub(super) fn semantic_type_ref<'a>(
    reference: &'a SemanticTypeRef,
    schema: &'a crate::schema::SemanticSchema,
) -> TypeView<'a> {
    super::type_view::from_semantic(reference, schema)
}
