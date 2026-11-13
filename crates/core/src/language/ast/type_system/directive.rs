//! Directive definitions and locations from the GraphQL specification.

use super::super::{Description, Name, Spanned};
use super::InputValueDefinition;
use crate::Span;

/// custom directive definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectiveDefinition {
    /// Description exposed through introspection.
    pub description: Option<Description>,
    /// Directive name without `@`.
    pub name: Name,
    /// Argument definitions in source order.
    pub arguments: Vec<InputValueDefinition>,
    /// Source range of the `repeatable` keyword, or `None` when absent.
    pub repeatable: Option<Span>,
    /// Locations in source order, separated by `|`.
    pub locations: Vec<Spanned<DirectiveLocation>>,
    /// Source range of the complete definition.
    pub span: Span,
}

impl DirectiveDefinition {
    /// Whether the directive may be applied multiple times at one location.
    pub const fn is_repeatable(&self) -> bool {
        self.repeatable.is_some()
    }
}

/// Location accepted in a directive definition.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DirectiveLocation {
    /// explicit query operation.
    Query,
    /// mutation operation.
    Mutation,
    /// subscription operation.
    Subscription,
    /// field selection.
    Field,
    /// fragment definition.
    FragmentDefinition,
    /// named fragment spread.
    FragmentSpread,
    /// inline fragment.
    InlineFragment,
    /// variable definition.
    VariableDefinition,
    /// Schema definition or extension.
    Schema,
    /// Scalar definition or extension.
    Scalar,
    /// Object definition or extension.
    Object,
    /// field definition.
    FieldDefinition,
    /// field argument definition.
    ArgumentDefinition,
    /// Interface definition or extension.
    Interface,
    /// Union definition or extension.
    Union,
    /// Enum definition or extension.
    Enum,
    /// enum value definition.
    EnumValue,
    /// Input object definition or extension.
    InputObject,
    /// input object field definition.
    InputFieldDefinition,
}

#[cfg(test)]
mod tests {
    use super::{DirectiveDefinition, DirectiveLocation};
    use crate::language::ast::{Name, Spanned};
    use crate::{SourceId, Span};

    fn span(start: usize, end: usize) -> Span {
        Span::new(SourceId::new(9), start, end)
    }

    #[test]
    fn preserves_repeatable_and_location_source_order() {
        let definition = DirectiveDefinition {
            description: None,
            name: Name::new("tag".to_owned(), span(11, 14)),
            arguments: Vec::new(),
            repeatable: Some(span(15, 25)),
            locations: vec![
                Spanned::new(DirectiveLocation::FieldDefinition, span(29, 45)),
                Spanned::new(DirectiveLocation::Object, span(48, 54)),
            ],
            span: span(0, 54),
        };

        assert!(definition.is_repeatable());
        assert_eq!(
            definition.locations[0].value,
            DirectiveLocation::FieldDefinition
        );
        assert_eq!(definition.locations[1].value, DirectiveLocation::Object);
    }
}
