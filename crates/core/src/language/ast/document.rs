//! Defines a document that maintains the executable definition and type-system definition in a mixed order.

use super::{ExecutableDefinition, TypeSystemDefinition};
use crate::Span;

/// Parsed document from one GraphQL source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Document {
    /// Executable or type-system definition preserved in source order.
    pub definitions: Vec<Definition>,
    /// Source span from the first definition through the last definition.
    pub span: Span,
}

impl Document {
    /// Iterate over executable definitions in source order.
    pub fn executable_definitions(&self) -> impl Iterator<Item = &ExecutableDefinition> {
        self.definitions
            .iter()
            .filter_map(|definition| match definition {
                Definition::Executable(executable) => Some(executable),
                Definition::TypeSystem(_) => None,
            })
    }

    /// Returns type-system definitions and extensions in source order.
    pub fn type_system_definitions(&self) -> impl Iterator<Item = &TypeSystemDefinition> {
        self.definitions
            .iter()
            .filter_map(|definition| match definition {
                Definition::Executable(_) => None,
                Definition::TypeSystem(type_system) => Some(type_system),
            })
    }
}

/// Two areas of definition recognized by common parser.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Definition {
    /// operation or fragment.
    Executable(ExecutableDefinition),
    /// schema/type/directive definition or extension.
    TypeSystem(TypeSystemDefinition),
}

#[cfg(test)]
mod tests {
    use super::{Definition, Document};
    use crate::language::ast::{
        ExecutableDefinition, Name, OperationDefinition, OperationType, ScalarTypeDefinition,
        SelectionSet, TypeSystemDefinition,
    };
    use crate::{SourceId, Span};

    fn span(start: usize, end: usize) -> Span {
        Span::new(SourceId::new(10), start, end)
    }

    #[test]
    fn mixed_documents_preserve_domain_and_source_order() {
        let scalar = TypeSystemDefinition::Scalar(ScalarTypeDefinition {
            description: None,
            name: Name::new("Date".to_owned(), span(7, 11)),
            directives: Vec::new(),
            span: span(0, 11),
        });
        let operation = ExecutableDefinition::Operation(OperationDefinition {
            description: None,
            operation_type: OperationType::Query,
            operation_type_span: Some(span(12, 17)),
            name: Some(Name::new("Today".to_owned(), span(18, 23))),
            variable_definitions: Vec::new(),
            directives: Vec::new(),
            selection_set: SelectionSet {
                selections: Vec::new(),
                span: span(24, 26),
            },
            span: span(12, 26),
        });
        let document = Document {
            definitions: vec![
                Definition::TypeSystem(scalar),
                Definition::Executable(operation),
            ],
            span: span(0, 26),
        };

        assert_eq!(document.type_system_definitions().count(), 1);
        assert_eq!(document.executable_definitions().count(), 1);
        assert!(matches!(document.definitions[0], Definition::TypeSystem(_)));
        assert!(matches!(document.definitions[1], Definition::Executable(_)));
    }
}
