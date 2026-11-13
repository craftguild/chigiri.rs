//! Store each definition once and represent graph edges and wrappers with resolved IDs.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::{Constraints, ScalarSpec};
use chigiri::core::Span;
use chigiri::core::language::ast::ValueNode;
use chigiri::core::schema::TypeKind;

/// Named type position in the graph.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct TypeId(pub usize);
/// Object or interface field position in the graph.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct FieldId(pub usize);
/// Input-field or argument index in the graph.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct InputId(pub usize);

/// GraphQL type wrapper, stored from outermost to innermost.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Wrapper {
    /// Non-null wrapper.
    NonNull,
    /// List wrapper.
    List,
}

/// A resolved named type plus its complete GraphQL wrapper sequence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeRef {
    /// Resolved named type.
    pub named: TypeId,
    /// Wrappers ordered from outermost to innermost.
    pub wrappers: Arc<[Wrapper]>,
}

impl TypeRef {
    /// Return whether the outermost value may be null.
    pub fn nullable(&self) -> bool {
        self.wrappers.first() != Some(&Wrapper::NonNull)
    }

    /// Return the number of nested list wrappers.
    pub fn list_depth(&self) -> usize {
        self.wrappers
            .iter()
            .filter(|w| **w == Wrapper::List)
            .count()
    }
}

/// One resolved named GraphQL type.
#[derive(Debug)]
pub struct TypeNode {
    /// Declared GraphQL name.
    pub name: Arc<str>,
    /// GraphQL kind.
    pub kind: TypeKind,
    /// Declaration location; built-in types have none.
    pub span: Option<Span>,
    /// Object or interface field-name index.
    pub fields: BTreeMap<Arc<str>, FieldId>,
    /// Input object field name index.
    pub inputs: BTreeMap<Arc<str>, InputId>,
    /// Non-null inputs and inputs with defaults that must be present after coercion.
    pub required_inputs: Vec<InputId>,
    /// Enum values in declaration order.
    pub enum_values: Vec<Arc<str>>,
    /// Name index used to avoid repeated linear enum scans at runtime.
    pub(crate) enum_index: BTreeSet<Arc<str>>,
    /// Interfaces explicitly implemented by this type.
    pub interfaces: Vec<TypeId>,
    /// Concrete object types possible for this object or abstract type.
    pub possible_types: Vec<TypeId>,
    /// Exclusive input condition for OneOf input.
    pub one_of: bool,
    /// Explicit representation and constraints for a custom scalar.
    pub scalar: Option<ScalarSpec>,
}

/// Resolved source field definition.
#[derive(Debug)]
pub struct FieldNode {
    /// Declaring object or interface.
    pub parent: TypeId,
    /// Declared field name.
    pub name: Arc<str>,
    /// The output type.
    pub ty: TypeRef,
    /// Arguments in source order.
    pub arguments: Vec<InputId>,
    /// Argument-name index used for bounded lookup.
    pub argument_names: BTreeMap<Arc<str>, InputId>,
    /// The original name location.
    pub span: Span,
    /// Constraints declared on the field itself.
    pub constraints: Constraints,
}

/// Resolved input field or argument in source order.
#[derive(Debug)]
pub struct InputNode {
    /// Declared input name.
    pub name: Arc<str>,
    /// Resolved input type.
    pub ty: TypeRef,
    /// Explicit default literal, including a distinct null literal.
    pub default: Option<ValueNode>,
    /// The original name location.
    pub span: Span,
    /// Constraints declared on the input field itself.
    pub constraints: Constraints,
}
