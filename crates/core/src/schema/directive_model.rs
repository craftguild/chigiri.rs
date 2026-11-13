//! Unified semantic directive model for built-in and source definitions.

use std::collections::BTreeMap;

use crate::Span;
use crate::language::ast::{
    Description, DirectiveDefinition, DirectiveLocation, InputValueDefinition, StringStyle,
    StringValue, TypeRef, TypeRefKind, Value,
};

use super::SchemaOrigin;

/// The semantic type reference with the source span arbitrarily.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticTypeRef {
    /// The structure of named/list/non-null.
    pub kind: SemanticTypeRefKind,
    /// Syntax span for source definitions, or `None` for built-ins.
    pub span: Option<Span>,
}

/// Semantic type-reference structure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SemanticTypeRefKind {
    /// Named type.
    Named(String),
    /// list item type.
    List(Box<SemanticTypeRef>),
    /// The wrapped type inside non-null.
    NonNull(Box<SemanticTypeRef>),
}

impl SemanticTypeRef {
    pub(super) fn from_source(reference: &TypeRef) -> Self {
        let kind = match &reference.kind {
            TypeRefKind::Named(name) => SemanticTypeRefKind::Named(name.value.clone()),
            TypeRefKind::List(item) => SemanticTypeRefKind::List(Box::new(Self::from_source(item))),
            TypeRefKind::NonNull(item) => {
                SemanticTypeRefKind::NonNull(Box::new(Self::from_source(item)))
            }
        };
        Self {
            kind,
            span: Some(reference.span),
        }
    }

    pub(super) fn named(name: &str) -> Self {
        Self {
            kind: SemanticTypeRefKind::Named(name.to_owned()),
            span: None,
        }
    }

    pub(super) fn non_null(inner: Self) -> Self {
        Self {
            kind: SemanticTypeRefKind::NonNull(Box::new(inner)),
            span: None,
        }
    }
}

/// The definition of a directive argument is defined by the source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticDirectiveArgument {
    /// The name of the argument.
    pub name: String,
    /// input type reference.
    pub ty: SemanticTypeRef,
    /// Default value; its source span is available through `source_definition`.
    pub default_value: Option<Value>,
    /// Whether this argument is built-in or source-defined.
    pub origin: SchemaOrigin,
    /// Lossless source definition, or `None` for built-ins.
    pub source_definition: Option<InputValueDefinition>,
}

/// The directive location and any source span.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticDirectiveLocation {
    /// GraphQL directive location.
    pub value: DirectiveLocation,
    /// Source location span, or `None` for built-ins.
    pub span: Option<Span>,
}

/// A directive definition after composition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticDirectiveDefinition {
    pub(super) name: String,
    pub(super) description: Option<Description>,
    pub(super) arguments: Vec<SemanticDirectiveArgument>,
    pub(super) argument_index: BTreeMap<String, usize>,
    pub(super) required_argument_indices: Vec<usize>,
    pub(super) repeatable: bool,
    pub(super) locations: Vec<SemanticDirectiveLocation>,
    pub(super) location_mask: u32,
    pub(super) origin: SchemaOrigin,
    pub(super) source_definition: Option<DirectiveDefinition>,
}

impl SemanticDirectiveDefinition {
    /// Returns the directive name without the leading `@`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the description.
    pub const fn description(&self) -> Option<&Description> {
        self.description.as_ref()
    }

    /// Returns argument definitions in source order.
    pub fn arguments(&self) -> &[SemanticDirectiveArgument] {
        &self.arguments
    }

    /// Looks up an argument definition by name.
    pub fn argument(&self, name: &str) -> Option<&SemanticDirectiveArgument> {
        self.argument_index
            .get(name)
            .and_then(|index| self.arguments.get(*index))
    }

    /// Returns whether the directive is repeatable.
    pub const fn is_repeatable(&self) -> bool {
        self.repeatable
    }

    /// Return the allowed locations in source order.
    pub fn locations(&self) -> &[SemanticDirectiveLocation] {
        &self.locations
    }

    pub(crate) fn allows_location(&self, location: DirectiveLocation) -> bool {
        self.location_mask & location_bit(location) != 0
    }

    /// Reverse the required argument in the schema definition order.
    pub(crate) fn required_arguments(
        &self,
    ) -> impl ExactSizeIterator<Item = &SemanticDirectiveArgument> {
        self.required_argument_indices
            .iter()
            .map(|index| &self.arguments[*index])
    }

    /// Returns whether this directive is built-in or source-defined.
    pub const fn origin(&self) -> SchemaOrigin {
        self.origin
    }

    /// Returns the lossless source definition, or `None` for built-ins.
    pub const fn source_definition(&self) -> Option<&DirectiveDefinition> {
        self.source_definition.as_ref()
    }

    pub(super) fn from_source(definition: &DirectiveDefinition) -> Self {
        let arguments = definition
            .arguments
            .iter()
            .map(argument_from_source)
            .collect::<Vec<_>>();
        let argument_index = arguments
            .iter()
            .enumerate()
            .map(|(index, argument)| (argument.name.clone(), index))
            .collect();
        let required_argument_indices = required_argument_indices(&arguments);
        let location_mask = definition
            .locations
            .iter()
            .fold(0, |mask, location| mask | location_bit(location.value));
        Self {
            name: definition.name.value.clone(),
            description: definition.description.clone(),
            arguments,
            argument_index,
            required_argument_indices,
            repeatable: definition.is_repeatable(),
            locations: definition
                .locations
                .iter()
                .map(|location| SemanticDirectiveLocation {
                    value: location.value,
                    span: Some(location.span),
                })
                .collect(),
            location_mask,
            origin: SchemaOrigin::Definition(definition.span),
            source_definition: Some(definition.clone()),
        }
    }
}

pub(super) const fn location_bit(location: DirectiveLocation) -> u32 {
    1_u32 << location as u32
}

pub(super) fn required_argument_indices(arguments: &[SemanticDirectiveArgument]) -> Vec<usize> {
    arguments
        .iter()
        .enumerate()
        .filter_map(|(index, argument)| {
            (matches!(argument.ty.kind, SemanticTypeRefKind::NonNull(_))
                && argument.default_value.is_none())
            .then_some(index)
        })
        .collect()
}

fn argument_from_source(argument: &InputValueDefinition) -> SemanticDirectiveArgument {
    SemanticDirectiveArgument {
        name: argument.name.value.clone(),
        ty: SemanticTypeRef::from_source(&argument.ty),
        default_value: argument
            .default_value
            .as_ref()
            .map(|value| value.value.clone()),
        origin: SchemaOrigin::Definition(argument.span),
        source_definition: Some(argument.clone()),
    }
}

pub(super) fn string_default(value: &str) -> Value {
    Value::String(StringValue::new(value, StringStyle::Quoted))
}
