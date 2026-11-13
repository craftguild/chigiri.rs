//! Defines executable-document syntax for operations, fragments, and selections.

use super::{Description, Directive, Name, OperationType, TypeRef, ValueNode};
use crate::Span;

/// Definition allowed in an executable document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutableDefinition {
    /// operation definition.
    Operation(OperationDefinition),
    /// fragment definition.
    Fragment(FragmentDefinition),
}

/// Query, mutation, or subscription operation definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationDefinition {
    /// Documentation description immediately before the operation.
    pub description: Option<Description>,
    /// Operation type.
    pub operation_type: OperationType,
    /// Source range of the explicit operation keyword, or `None` for shorthand
    /// queries.
    pub operation_type_span: Option<Span>,
    /// Name of an explicit operation.
    pub name: Option<Name>,
    /// Variable definitions in source order.
    pub variable_definitions: Vec<VariableDefinition>,
    /// Directive applications in source order.
    pub directives: Vec<Directive>,
    /// root selection set.
    pub selection_set: SelectionSet,
    /// Source range of the complete operation definition.
    pub span: Span,
}

impl OperationDefinition {
    /// Whether this operation was parsed as a shorthand query.
    pub const fn is_shorthand(&self) -> bool {
        self.operation_type_span.is_none()
    }
}

/// Variable declared by an operation with its input type and default.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VariableDefinition {
    /// Documentation description immediately before the variable definition.
    pub description: Option<Description>,
    /// Variable name without `$`.
    pub variable: Name,
    /// Declared input type reference.
    pub ty: TypeRef,
    /// Optional default value.
    pub default_value: Option<ValueNode>,
    /// Directives applied to the variable definition.
    pub directives: Vec<Directive>,
    /// Source range from the description or `$` through the end of the definition.
    pub span: Span,
}

/// named fragment definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FragmentDefinition {
    /// Documentation description immediately before the fragment.
    pub description: Option<Description>,
    /// Fragment name.
    pub name: Name,
    /// Type condition following `on`.
    pub type_condition: Name,
    /// Directives applied to the fragment definition.
    pub directives: Vec<Directive>,
    /// Fragment selection set.
    pub selection_set: SelectionSet,
    /// Source range of the complete definition.
    pub span: Span,
}

/// Sequence of selections including its braces.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectionSet {
    /// Selections in source order.
    pub selections: Vec<Selection>,
    /// Source range from the opening through closing brace.
    pub span: Span,
}

/// Selection allowed in a selection set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Selection {
    /// field selection.
    Field(Field),
    /// named fragment spread.
    FragmentSpread(FragmentSpread),
    /// inline fragment.
    InlineFragment(InlineFragment),
}

/// Field selection with an optional alias, arguments, and subselection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Field {
    /// Alias that changes the response key.
    pub alias: Option<Name>,
    /// Field name in the schema.
    pub name: Name,
    /// Arguments in source order.
    pub arguments: Vec<super::Argument>,
    /// Directive applications in source order.
    pub directives: Vec<Directive>,
    /// Subselection written for a composite field.
    pub selection_set: Option<SelectionSet>,
    /// Source range of the complete field selection.
    pub span: Span,
}

/// Fragment spread in the `...FragmentName` form.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FragmentSpread {
    /// Name of the referenced fragment.
    pub name: Name,
    /// Directives applied at the spread location.
    pub directives: Vec<Directive>,
    /// Source range from the spread token through the final directive.
    pub span: Span,
}

/// Inline fragment in `... on Type` form, optionally without a type condition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineFragment {
    /// Type condition when `on` is present.
    pub type_condition: Option<Name>,
    /// Directives applied to the inline fragment.
    pub directives: Vec<Directive>,
    /// Inline-fragment selection set.
    pub selection_set: SelectionSet,
    /// Source range from the spread token through the selection set.
    pub span: Span,
}

#[cfg(test)]
mod tests {
    use super::{
        Field, FragmentDefinition, OperationDefinition, Selection, SelectionSet, VariableDefinition,
    };
    use crate::language::ast::{
        Description, Name, OperationType, StringStyle, StringValue, TypeRef,
    };
    use crate::{SourceId, Span};

    fn span(start: usize, end: usize) -> Span {
        Span::new(SourceId::new(2), start, end)
    }

    #[test]
    fn represents_shorthand_queries_without_a_keyword_span() {
        let field = Field {
            alias: Some(Name::new("viewer".to_owned(), span(2, 8))),
            name: Name::new("user".to_owned(), span(10, 14)),
            arguments: Vec::new(),
            directives: Vec::new(),
            selection_set: None,
            span: span(2, 14),
        };
        let selection_set = SelectionSet {
            selections: vec![Selection::Field(field)],
            span: span(0, 16),
        };
        let operation = OperationDefinition {
            description: None,
            operation_type: OperationType::Query,
            operation_type_span: None,
            name: None,
            variable_definitions: Vec::new(),
            directives: Vec::new(),
            selection_set,
            span: span(0, 16),
        };

        assert!(operation.is_shorthand());
        let Selection::Field(field) = &operation.selection_set.selections[0] else {
            panic!("expected a field selection");
        };
        assert_eq!(
            field.alias.as_ref().map(|name| name.value.as_str()),
            Some("viewer")
        );
    }

    #[test]
    fn parses_subscriptions_independently_of_execution_support() {
        let operation = OperationDefinition {
            description: None,
            operation_type: OperationType::Subscription,
            operation_type_span: Some(span(0, 12)),
            name: Some(Name::new("Events".to_owned(), span(13, 19))),
            variable_definitions: Vec::new(),
            directives: Vec::new(),
            selection_set: SelectionSet {
                selections: Vec::new(),
                span: span(20, 22),
            },
            span: span(0, 22),
        };

        assert!(!operation.is_shorthand());
        assert_eq!(operation.operation_type, OperationType::Subscription);
    }

    #[test]
    fn preserves_descriptions_on_each_named_definition() {
        let description = |text: &str, start: usize| {
            Description::new(
                StringValue::new(text, StringStyle::Quoted),
                span(start, start + text.len() + 2),
            )
        };
        let variable = VariableDefinition {
            description: Some(description("machine", 10)),
            variable: Name::new("id".to_owned(), span(22, 24)),
            ty: TypeRef::named(Name::new("ID".to_owned(), span(26, 28)), span(26, 28)),
            default_value: None,
            directives: Vec::new(),
            span: span(10, 28),
        };
        let operation = OperationDefinition {
            description: Some(description("status", 0)),
            operation_type: OperationType::Query,
            operation_type_span: Some(span(8, 13)),
            name: Some(Name::new("Status".to_owned(), span(14, 20))),
            variable_definitions: vec![variable],
            directives: Vec::new(),
            selection_set: SelectionSet {
                selections: Vec::new(),
                span: span(29, 31),
            },
            span: span(0, 31),
        };
        let fragment = FragmentDefinition {
            description: Some(description("details", 32)),
            name: Name::new("Details".to_owned(), span(43, 50)),
            type_condition: Name::new("Machine".to_owned(), span(54, 61)),
            directives: Vec::new(),
            selection_set: SelectionSet {
                selections: Vec::new(),
                span: span(62, 64),
            },
            span: span(32, 64),
        };

        assert_eq!(operation.description.unwrap().value.value, "status");
        assert_eq!(
            operation.variable_definitions[0]
                .description
                .as_ref()
                .unwrap()
                .value
                .value,
            "machine"
        );
        assert_eq!(fragment.description.unwrap().value.value, "details");
    }
}
