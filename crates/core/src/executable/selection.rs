//! The validated document determines the requested operation.

use std::fmt;

use crate::DiagnosticCode;
use crate::language::ast::{Definition, ExecutableDefinition};

use super::{SelectedOperation, ValidatedDocument};

/// Multiple operations require an explicit operation name.
pub const OPERATION_NAME_REQUIRED: DiagnosticCode = DiagnosticCode::new("SF3090");
/// There is no operation name specified in the request.
pub const OPERATION_NOT_FOUND: DiagnosticCode = DiagnosticCode::new("SF3091");
/// Legacy diagnostic code for the former subscription-operation rejection.
pub const OPERATION_NOT_SUPPORTED: DiagnosticCode = DiagnosticCode::new("SF3092");

/// Request error that causes failure before the operation selection starts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationSelectionError {
    /// Stability code for machine detection.
    pub code: DiagnosticCode,
    /// Diagnostic message that is not assumed to be safe for clients.
    pub message: String,
}

impl fmt::Display for OperationSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for OperationSelectionError {}

pub(super) fn select<'document>(
    document: &'document ValidatedDocument,
    operation_name: Option<&str>,
) -> Result<SelectedOperation<'document>, OperationSelectionError> {
    let index = match operation_name {
        Some(name) => document
            .named_operations
            .get(name)
            .copied()
            .ok_or_else(|| OperationSelectionError {
                code: OPERATION_NOT_FOUND,
                message: format!("Operation `{name}` was not found"),
            })?,
        None if document.operation_indices.len() == 1 => document.operation_indices[0],
        None => {
            return Err(OperationSelectionError {
                code: OPERATION_NAME_REQUIRED,
                message: "Operation name is required when a document has multiple operations"
                    .to_owned(),
            });
        }
    };
    let Some(Definition::Executable(ExecutableDefinition::Operation(operation))) =
        document.document.definitions.get(index)
    else {
        return Err(OperationSelectionError {
            code: OPERATION_NOT_FOUND,
            message: "Selected operation is not available".to_owned(),
        });
    };
    Ok(SelectedOperation {
        operation,
        complexity: document
            .complexities
            .get(&index)
            .copied()
            .unwrap_or_default(),
    })
}
