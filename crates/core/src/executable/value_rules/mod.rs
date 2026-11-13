//! Static validation of executable literal shapes and variable-use positions.

mod builtin;
mod catalog;
mod custom;
mod literal;
#[cfg(test)]
pub(super) mod metrics;
mod required;
mod types;
mod uses;
mod walk;

use crate::language::ast::Document;
use crate::schema::SemanticSchema;
use crate::{DiagnosticBag, DiagnosticCode, ExecutionLimits};

use super::{
    CustomScalarRegistry, INPUT_CUSTOM_SCALAR_VALUE, INPUT_DUPLICATE_FIELD, INPUT_ENUM_VALUE,
    INPUT_NULL_FOR_NON_NULL, INPUT_OBJECT_VALUE, INPUT_ONE_OF_VALUE, INPUT_REQUIRED_VALUE_MISSING,
    INPUT_SCALAR_VALUE, INPUT_UNKNOWN_FIELD,
};

/// A variable definition type is incompatible with its usage location.
pub(super) const VARIABLE_POSITION_INVALID: DiagnosticCode = DiagnosticCode::new("SF3071");

/// Validates allowed variable positions for each value shape and operation.
pub(super) fn validate_values(
    schema: &SemanticSchema,
    document: &Document,
    scalars: &CustomScalarRegistry,
    limits: &ExecutionLimits,
    diagnostics: &mut DiagnosticBag,
) -> bool {
    let catalog = catalog::Catalog::new(schema);
    let mut inspector = literal::Inspector::new(schema, scalars, limits, diagnostics);
    if !walk::validate_static(schema, document, &catalog, &mut inspector) {
        return false;
    }
    let (literal_valid, sites) = inspector.finish();
    let positions_valid = uses::validate_positions(document, &sites, diagnostics);
    literal_valid && positions_valid
}
