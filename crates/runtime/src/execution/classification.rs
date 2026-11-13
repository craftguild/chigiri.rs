//! Parse-only routing hints; validation and application coercion remain separate.

use chigiri_core::language::ast::{ExecutableDefinition, OperationType};
use chigiri_core::language::parse_executable_document;
use chigiri_core::{ExecutionLimits, GraphQlRequest, SourceId};

use crate::ExecutableSchema;

/// A uniquely selected syntactic operation, never proof that execution is valid.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectedHttpOperation {
    /// One selected query, including shorthand syntax.
    Query,
    /// One selected mutation; safe-GET enforcement remains in the finite executor.
    Mutation,
    /// One selected subscription; complete preparation must still validate it.
    Subscription,
    /// Parsing, parser limits, or operation selection prevent a positive hint.
    Indeterminate,
}

impl<C: Send + Sync + 'static> ExecutableSchema<C> {
    /// Classifies only complete bounded syntax after operation-source resolution.
    ///
    /// This method runs no schema validation, scalar callback, plugin, context
    /// factory, resolver, diagnostics hook, or document-cache operation. A caller
    /// must still use normal finite execution or full subscription preparation;
    /// an indeterminate hint must never authorize source setup. An explicitly
    /// selected stream-only profile may still prepare a bounded request error.
    #[doc(hidden)]
    pub fn __selected_http_operation(&self, request: &GraphQlRequest) -> SelectedHttpOperation {
        classify(
            &request.query,
            request.operation_name.as_deref(),
            &self.inner.limits,
        )
    }
}

fn classify(
    source: &str,
    requested: Option<&str>,
    limits: &ExecutionLimits,
) -> SelectedHttpOperation {
    let parsed = parse_executable_document(SourceId::new(0), source, limits);
    let Some(document) = parsed.document else {
        return SelectedHttpOperation::Indeterminate;
    };
    let mut selected = None;
    let mut operations = 0;
    let mut anonymous = false;
    for definition in document.executable_definitions() {
        let ExecutableDefinition::Operation(operation) = definition else {
            continue;
        };
        operations += 1;
        let name = operation.name.as_ref().map(|name| name.value.as_str());
        anonymous |= name.is_none();
        if requested.is_none() || requested == name {
            if selected.is_some() {
                return SelectedHttpOperation::Indeterminate;
            }
            selected = Some(operation.operation_type);
        }
    }
    // A mixed anonymous document is never a safely selectable operation, even
    // when a named sibling would otherwise match the requested name.
    if anonymous && operations != 1 {
        return SelectedHttpOperation::Indeterminate;
    }
    match selected {
        Some(OperationType::Query) => SelectedHttpOperation::Query,
        Some(OperationType::Mutation) => SelectedHttpOperation::Mutation,
        Some(OperationType::Subscription) => SelectedHttpOperation::Subscription,
        None => SelectedHttpOperation::Indeterminate,
    }
}

#[cfg(test)]
mod tests;
