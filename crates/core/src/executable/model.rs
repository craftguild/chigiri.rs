//! Validated document model reused by request execution and the cache.

use std::collections::BTreeMap;

use crate::language::ast::{Definition, Document, FragmentDefinition, OperationDefinition};

use super::OperationSelectionError;

/// Operation resource counts after fragment expansion.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ComplexitySummary {
    /// Counts the root selection as depth one.
    pub depth: usize,
    /// The total number of field selections after fragment expansion.
    pub expanded_fields: usize,
    /// The total number of alias after fragment expansion.
    pub aliases: usize,
    /// The number of variables that the operation declares.
    pub variables: usize,
    /// Counts directive applications across the operation and all selections.
    pub directives: usize,
}

/// An owned AST that has passed full document validation for the schema.
#[derive(Clone, Debug)]
pub struct ValidatedDocument {
    pub(super) document: Document,
    pub(super) operation_indices: Vec<usize>,
    pub(super) named_operations: BTreeMap<String, usize>,
    pub(super) fragment_indices: BTreeMap<String, usize>,
    pub(super) complexities: BTreeMap<usize, ComplexitySummary>,
}

impl ValidatedDocument {
    /// Returns the parsed document that has been preserved.
    pub const fn document(&self) -> &Document {
        &self.document
    }

    /// Iterates over operations in document order.
    pub fn operations(&self) -> impl Iterator<Item = &OperationDefinition> {
        self.operation_indices.iter().filter_map(|index| {
            let Definition::Executable(definition) = self.document.definitions.get(*index)? else {
                return None;
            };
            let crate::language::ast::ExecutableDefinition::Operation(operation) = definition
            else {
                return None;
            };
            Some(operation)
        })
    }

    /// Looks up a fragment by name.
    pub fn fragment(&self, name: &str) -> Option<&FragmentDefinition> {
        let index = *self.fragment_indices.get(name)?;
        let Definition::Executable(crate::language::ast::ExecutableDefinition::Fragment(fragment)) =
            self.document.definitions.get(index)?
        else {
            return None;
        };
        Some(fragment)
    }

    /// Selects an operation using the request's operation-name rules.
    pub fn select_operation(
        &self,
        operation_name: Option<&str>,
    ) -> Result<SelectedOperation<'_>, OperationSelectionError> {
        super::selection::select(self, operation_name)
    }
}

/// The per-request selected operation and its validated resource counts.
#[derive(Clone, Copy, Debug)]
pub struct SelectedOperation<'document> {
    /// The selected operation definition.
    pub operation: &'document OperationDefinition,
    /// Resource totals including fragment expansion.
    pub complexity: ComplexitySummary,
}
