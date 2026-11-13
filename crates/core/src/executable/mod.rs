//! Executable-document validation, operation selection, and input coercion.

mod ast_invariants;
mod complexity;
mod field_rules;
mod fragment_rules;
mod input_coercion;
mod model;
mod selection;
mod validation;
mod value_rules;
mod variable_rules;

use crate::language::ast::Document;
use crate::schema::SemanticSchema;
use crate::{DiagnosticBag, ExecutionLimits};

pub use input_coercion::{
    CoercedInput, CoercedVariables, CustomScalarRegistry, INPUT_CUSTOM_SCALAR_VALUE,
    INPUT_DUPLICATE_FIELD, INPUT_ENUM_VALUE, INPUT_NESTING_LIMIT, INPUT_NULL_FOR_NON_NULL,
    INPUT_OBJECT_VALUE, INPUT_ONE_OF_VALUE, INPUT_REQUIRED_VALUE_MISSING, INPUT_RESOURCE_LIMIT,
    INPUT_SCALAR_VALUE, INPUT_UNKNOWN_FIELD, INPUT_VARIABLE_UNAVAILABLE, InputCoercionBudget,
    InputCoercionOutput, ScalarCoercionError, VariableCoercionOutput, coerce_argument_value,
    coerce_argument_with_budget, coerce_literal_input, coerce_literal_with_budget,
    coerce_raw_input, coerce_raw_with_budget, coerce_variable_values, coerce_variables_with_budget,
};
pub use model::{ComplexitySummary, SelectedOperation, ValidatedDocument};
pub use selection::{
    OPERATION_NAME_REQUIRED, OPERATION_NOT_FOUND, OPERATION_NOT_SUPPORTED, OperationSelectionError,
};

/// An executable document contains a type-system definition.
pub const EXECUTABLE_TYPE_SYSTEM_DEFINITION: crate::DiagnosticCode =
    crate::DiagnosticCode::new("SF3001");
/// The name of the operation is duplicated in the document.
pub const EXECUTABLE_DUPLICATE_OPERATION_NAME: crate::DiagnosticCode =
    crate::DiagnosticCode::new("SF3002");
/// An anonymous operation appears in a document with multiple operations.
pub const EXECUTABLE_LONE_ANONYMOUS_OPERATION: crate::DiagnosticCode =
    crate::DiagnosticCode::new("SF3003");
/// The fragment name was duplicated in the document.
pub const EXECUTABLE_DUPLICATE_FRAGMENT_NAME: crate::DiagnosticCode =
    crate::DiagnosticCode::new("SF3004");
/// The public AST violates the GraphQL parser's syntax invariants.
pub const EXECUTABLE_INVALID_AST_SYNTAX: crate::DiagnosticCode =
    crate::DiagnosticCode::new("SF3005");
/// The public AST's recursive structure exceeds the parser nesting limit.
pub const EXECUTABLE_AST_NESTING_LIMIT: crate::DiagnosticCode =
    crate::DiagnosticCode::new("SF3006");
/// The type condition of a fragment is not a defined composite type.
pub const EXECUTABLE_FRAGMENT_INVALID_TYPE_CONDITION: crate::DiagnosticCode =
    fragment_rules::FRAGMENT_INVALID_TYPE_CONDITION;
/// A fragment spread's target does not exist in the document.
pub const EXECUTABLE_FRAGMENT_SPREAD_TARGET_UNDEFINED: crate::DiagnosticCode =
    fragment_rules::FRAGMENT_SPREAD_TARGET_UNDEFINED;
/// The fragment spread graph contains a cycle.
pub const EXECUTABLE_FRAGMENT_SPREAD_CYCLE: crate::DiagnosticCode =
    fragment_rules::FRAGMENT_SPREAD_CYCLE;
/// The fragment cannot be reached by any operation.
pub const EXECUTABLE_FRAGMENT_UNUSED: crate::DiagnosticCode = fragment_rules::FRAGMENT_UNUSED;
/// A fragment spread has no object types in common with its parent selection.
pub const EXECUTABLE_FRAGMENT_SPREAD_NOT_POSSIBLE: crate::DiagnosticCode =
    fragment_rules::FRAGMENT_SPREAD_NOT_POSSIBLE;
/// One operation defines the same variable more than once.
pub const EXECUTABLE_DUPLICATE_VARIABLE: crate::DiagnosticCode =
    crate::DiagnosticCode::new("SF3020");
/// A variable definition refers to a non-input type.
pub const EXECUTABLE_VARIABLE_NOT_INPUT: crate::DiagnosticCode =
    crate::DiagnosticCode::new("SF3021");
/// The operation or a reachable fragment uses an undefined variable.
pub const EXECUTABLE_VARIABLE_UNDEFINED: crate::DiagnosticCode =
    crate::DiagnosticCode::new("SF3022");
/// A variable defined by the operation is not used by a reachable selection.
pub const EXECUTABLE_VARIABLE_UNUSED: crate::DiagnosticCode = crate::DiagnosticCode::new("SF3023");
/// The variable definition of the operation is above the set limit.
pub const EXECUTABLE_VARIABLE_LIMIT: crate::DiagnosticCode = crate::DiagnosticCode::new("SF3024");
/// The schema has no root object for the operation type.
pub const EXECUTABLE_ROOT_TYPE_MISSING: crate::DiagnosticCode = field_rules::ROOT_TYPE_MISSING;
/// The field of selection is not defined by the parent type.
pub const EXECUTABLE_FIELD_NOT_DEFINED: crate::DiagnosticCode = field_rules::FIELD_NOT_DEFINED;
/// The composite field has no selection set.
pub const EXECUTABLE_FIELD_SELECTION_REQUIRED: crate::DiagnosticCode =
    field_rules::FIELD_SELECTION_REQUIRED;
/// The leaf field has a selection set.
pub const EXECUTABLE_LEAF_SELECTION_PRESENT: crate::DiagnosticCode =
    field_rules::LEAF_SELECTION_PRESENT;
/// A field received an undefined argument.
pub const EXECUTABLE_FIELD_ARGUMENT_UNKNOWN: crate::DiagnosticCode =
    field_rules::FIELD_ARGUMENT_UNKNOWN;
/// A field application repeats an argument.
pub const EXECUTABLE_FIELD_ARGUMENT_DUPLICATE: crate::DiagnosticCode =
    field_rules::FIELD_ARGUMENT_DUPLICATE;
/// The required argument for the field is not specified.
pub const EXECUTABLE_FIELD_ARGUMENT_REQUIRED: crate::DiagnosticCode =
    field_rules::FIELD_ARGUMENT_REQUIRED;
/// A subscription operation does not collect exactly one root response-name group.
pub const EXECUTABLE_SUBSCRIPTION_SINGLE_ROOT_FIELD: crate::DiagnosticCode =
    field_rules::SUBSCRIPTION_SINGLE_ROOT_FIELD;
/// A subscription root selection uses `@skip` or `@include`.
pub const EXECUTABLE_SUBSCRIPTION_ROOT_DIRECTIVE: crate::DiagnosticCode =
    field_rules::SUBSCRIPTION_ROOT_DIRECTIVE;
/// A subscription operation selects an introspection field at its root.
pub const EXECUTABLE_SUBSCRIPTION_INTROSPECTION_ROOT: crate::DiagnosticCode =
    field_rules::SUBSCRIPTION_INTROSPECTION_ROOT;
/// The field of the same response name has an incompatible return shape.
pub const EXECUTABLE_FIELD_MERGE_RETURN_SHAPE: crate::DiagnosticCode =
    crate::DiagnosticCode::new("SF3036");
/// Fields with the same response name have different field names.
pub const EXECUTABLE_FIELD_MERGE_NAME: crate::DiagnosticCode = crate::DiagnosticCode::new("SF3037");
/// Fields with the same name have different arguments.
pub const EXECUTABLE_FIELD_MERGE_ARGUMENTS: crate::DiagnosticCode =
    crate::DiagnosticCode::new("SF3038");
/// The executable directive is not defined in the schema.
pub const EXECUTABLE_DIRECTIVE_UNKNOWN: crate::DiagnosticCode =
    crate::DiagnosticCode::new("SF3040");
/// The directive does not allow the current executable location.
pub const EXECUTABLE_DIRECTIVE_LOCATION_INVALID: crate::DiagnosticCode =
    crate::DiagnosticCode::new("SF3041");
/// A non-repeatable directive appears more than once at the same location.
pub const EXECUTABLE_DIRECTIVE_DUPLICATE: crate::DiagnosticCode =
    crate::DiagnosticCode::new("SF3042");
/// The directive argument is duplicated in one application.
pub const EXECUTABLE_DIRECTIVE_ARGUMENT_DUPLICATE: crate::DiagnosticCode =
    crate::DiagnosticCode::new("SF3043");
/// The directive was given an undefined argument.
pub const EXECUTABLE_DIRECTIVE_ARGUMENT_UNKNOWN: crate::DiagnosticCode =
    crate::DiagnosticCode::new("SF3044");
/// The directive's required argument is not specified.
pub const EXECUTABLE_DIRECTIVE_ARGUMENT_REQUIRED: crate::DiagnosticCode =
    crate::DiagnosticCode::new("SF3045");
/// The operation depth after fragment expansion exceeds the configured limit.
pub const EXECUTABLE_DEPTH_LIMIT: crate::DiagnosticCode = crate::DiagnosticCode::new("SF3050");
/// The total number of fields after fragment expansion is above the set limit.
pub const EXECUTABLE_FIELD_LIMIT: crate::DiagnosticCode = crate::DiagnosticCode::new("SF3051");
/// The total number of aliases after fragment expansion exceeds the configured limit.
pub const EXECUTABLE_ALIAS_LIMIT: crate::DiagnosticCode = crate::DiagnosticCode::new("SF3052");
/// The total number of directives after fragment expansion exceeds the configured limit.
pub const EXECUTABLE_DIRECTIVE_LIMIT: crate::DiagnosticCode = crate::DiagnosticCode::new("SF3053");
/// A variable definition type is incompatible with its usage location.
pub const EXECUTABLE_VARIABLE_POSITION_INVALID: crate::DiagnosticCode =
    value_rules::VARIABLE_POSITION_INVALID;

/// Executable validation result and bounded diagnostics.
#[derive(Clone, Debug)]
pub struct ExecutableValidationOutput {
    /// Reusable document returned only when every validation rule succeeds.
    pub document: Option<ValidatedDocument>,
    /// Diagnostics in deterministic phase, rule, AST, and schema order.
    pub diagnostics: DiagnosticBag,
    diagnostics_truncated: bool,
}

impl ExecutableValidationOutput {
    /// Returns whether validation produced an executable document.
    pub const fn is_valid(&self) -> bool {
        self.document.is_some()
    }

    /// Return whether validation reached the diagnostic limit.
    pub const fn diagnostics_were_truncated(&self) -> bool {
        self.diagnostics_truncated
    }
}

/// Parsed document that may be reused only after successful schema validation.
pub fn validate_executable_document(
    schema: &SemanticSchema,
    document: &Document,
    limits: &ExecutionLimits,
) -> ExecutableValidationOutput {
    validate_executable_document_with_scalars(
        schema,
        document,
        &CustomScalarRegistry::new(),
        limits,
    )
}

/// Validates a parsed document with custom-scalar callbacks.
pub fn validate_executable_document_with_scalars(
    schema: &SemanticSchema,
    document: &Document,
    scalars: &CustomScalarRegistry,
    limits: &ExecutionLimits,
) -> ExecutableValidationOutput {
    validation::validate(schema, document, scalars, limits)
}

#[cfg(test)]
mod tests_complexity;
#[cfg(test)]
mod tests_definitions;
#[cfg(test)]
mod tests_fragments;
#[cfg(test)]
mod tests_subscriptions;
#[cfg(test)]
mod tests_value_limits;
#[cfg(test)]
mod tests_value_rules;
#[cfg(test)]
mod tests_variables;
