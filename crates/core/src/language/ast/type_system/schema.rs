//! Definition and extension syntax for explicit schema roots.

use super::super::{Description, Directive, Name, OperationType, Spanned};
use crate::Span;

/// Explicit schema definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaDefinition {
    /// Schema description.
    pub description: Option<Description>,
    /// The directive applied to the schema.
    pub directives: Vec<Directive>,
    /// Root operation mapping.
    pub root_operations: Vec<RootOperationTypeDefinition>,
    /// Source span of the complete definition.
    pub span: Span,
}

/// Extension to existing schema.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaExtension {
    /// The directive applied to the extension.
    pub directives: Vec<Directive>,
    /// Root operation mapping.
    pub root_operations: Vec<RootOperationTypeDefinition>,
    /// Source range from `extend` through the end of the extension.
    pub span: Span,
}

/// Root object type for an operation kind.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootOperationTypeDefinition {
    /// Spanned `query`, `mutation`, or `subscription` token.
    pub operation_type: Spanned<OperationType>,
    /// The root object type name.
    pub named_type: Name,
    /// The source range from the operation token to the type name end.
    pub span: Span,
}

#[cfg(test)]
mod tests {
    use super::{RootOperationTypeDefinition, SchemaExtension};
    use crate::language::ast::{Name, OperationType, Spanned};
    use crate::{SourceId, Span};

    fn span(start: usize, end: usize) -> Span {
        Span::new(SourceId::new(6), start, end)
    }

    #[test]
    fn schema_extensions_can_define_subscription_roots() {
        let root = RootOperationTypeDefinition {
            operation_type: Spanned::new(OperationType::Subscription, span(16, 28)),
            named_type: Name::new("Events".to_owned(), span(30, 36)),
            span: span(16, 36),
        };
        let extension = SchemaExtension {
            directives: Vec::new(),
            root_operations: vec![root],
            span: span(0, 38),
        };

        assert_eq!(
            extension.root_operations[0].operation_type.value,
            OperationType::Subscription
        );
    }
}
