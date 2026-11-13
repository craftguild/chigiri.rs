//! GraphQL request-input coercion.

mod bounded;
mod budget;
mod builtin;
mod diagnostics;
mod engine;
mod entry;
mod objects;
mod plan;
mod runtime_value;
mod scalar;
mod variables;

use std::collections::BTreeMap;

use crate::language::ast::{InputValueDefinition, TypeRef, ValueNode};
use crate::schema::SemanticSchema;
use crate::{DiagnosticBag, DiagnosticCode, ExecutionLimits, Value};

pub use bounded::{
    coerce_argument_with_budget, coerce_literal_with_budget, coerce_raw_with_budget,
};
pub use budget::InputCoercionBudget;
pub use scalar::{CustomScalarRegistry, ScalarCoercionError};

/// Null was supplied for non-null input.
pub const INPUT_NULL_FOR_NON_NULL: DiagnosticCode = DiagnosticCode::new("SF3060");
/// Input does not satisfy a built-in scalar's type or range constraints.
pub const INPUT_SCALAR_VALUE: DiagnosticCode = DiagnosticCode::new("SF3061");
/// Enum input is not one of the defined values.
pub const INPUT_ENUM_VALUE: DiagnosticCode = DiagnosticCode::new("SF3062");
/// Input-object input is not an object value.
pub const INPUT_OBJECT_VALUE: DiagnosticCode = DiagnosticCode::new("SF3063");
/// An input object contains an undefined field.
pub const INPUT_UNKNOWN_FIELD: DiagnosticCode = DiagnosticCode::new("SF3064");
/// A required argument, variable, or input field is missing.
pub const INPUT_REQUIRED_VALUE_MISSING: DiagnosticCode = DiagnosticCode::new("SF3065");
/// An input-object literal specifies the same field more than once.
pub const INPUT_DUPLICATE_FIELD: DiagnosticCode = DiagnosticCode::new("SF3066");
/// OneOf input does not meet the exactly-one or non-null constraints.
pub const INPUT_ONE_OF_VALUE: DiagnosticCode = DiagnosticCode::new("SF3067");
/// A custom-scalar callback rejected the input or no callback is registered.
pub const INPUT_CUSTOM_SCALAR_VALUE: DiagnosticCode = DiagnosticCode::new("SF3068");
/// An input value or type reference exceeds the configured nesting limit.
pub const INPUT_NESTING_LIMIT: DiagnosticCode = DiagnosticCode::new("SF3069");
/// A literal refers to an undefined variable or one that failed coercion.
pub const INPUT_VARIABLE_UNAVAILABLE: DiagnosticCode = DiagnosticCode::new("SF3070");

/// The owned request exceeded the coercion work budget.
pub const INPUT_RESOURCE_LIMIT: DiagnosticCode = DiagnosticCode::new("SF3071");

/// Coercion result that distinguishes omitted GraphQL input from explicit null.
#[derive(Clone, Debug, PartialEq)]
pub enum CoercedInput {
    /// Omitted argument, variable, or input field.
    Undefined,
    /// Explicit null.
    Null,
    /// Coerced non-null value.
    Value(Value),
}

impl CoercedInput {
    /// Convert explicit null and concrete values to core [`Value`].
    pub fn into_value(self) -> Option<Value> {
        match self {
            Self::Undefined => None,
            Self::Null => Some(Value::Null),
            Self::Value(value) => Some(value),
        }
    }
}

/// Per-request lookup for variables required by coercion.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoercedVariables {
    values: BTreeMap<String, CoercedInput>,
}

impl CoercedVariables {
    /// Creates an empty variable lookup.
    pub const fn new() -> Self {
        Self {
            values: BTreeMap::new(),
        }
    }

    /// Returns the coerced value for a variable name without `$`.
    pub fn get(&self, name: &str) -> Option<&CoercedInput> {
        self.values.get(name)
    }

    /// Iterates in variable-name order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &CoercedInput)> {
        self.values
            .iter()
            .map(|(name, value)| (name.as_str(), value))
    }

    pub(super) fn insert(&mut self, name: String, value: CoercedInput) {
        self.values.insert(name, value);
    }
}

/// Result of one literal or raw-input coercion operation.
#[derive(Clone, Debug)]
pub struct InputCoercionOutput {
    /// A present value is returned only when all coercion rules succeed.
    pub value: Option<CoercedInput>,
    /// Client-safe diagnostics collected within the configured limit.
    pub diagnostics: DiagnosticBag,
    diagnostics_truncated: bool,
}

impl InputCoercionOutput {
    /// Return the coerced value when coercion completes without diagnostics.
    pub const fn is_valid(&self) -> bool {
        self.value.is_some()
    }

    /// Returns whether coercion reached the diagnostic limit.
    pub const fn diagnostics_were_truncated(&self) -> bool {
        self.diagnostics_truncated
    }
}

/// Coerces a raw transport value to a schema input type.
///
/// Bounds value and type traversal by `limits.max_parser_nesting`.
/// `unbounded()` is used only when tests or tooling explicitly remove this limit.
pub fn coerce_raw_input(
    schema: &SemanticSchema,
    expected: &TypeRef,
    raw: Value,
    scalars: &CustomScalarRegistry,
    limits: &ExecutionLimits,
) -> InputCoercionOutput {
    entry::coerce_raw(schema, expected, raw, scalars, limits, None)
}

/// Validate a literal that already contains substituted variables, then coerce it.
///
/// Document validation is a contract that guarantees parser nesting and variable position first.
pub fn coerce_literal_input(
    schema: &SemanticSchema,
    expected: &TypeRef,
    literal: &ValueNode,
    variables: &CoercedVariables,
    scalars: &CustomScalarRegistry,
    limits: &ExecutionLimits,
) -> InputCoercionOutput {
    entry::coerce_literal(schema, expected, literal, variables, scalars, limits, None)
}

/// Coerces an argument using variable presence and argument defaults.
pub fn coerce_argument_value(
    schema: &SemanticSchema,
    definition: &InputValueDefinition,
    provided: Option<&ValueNode>,
    variables: &CoercedVariables,
    scalars: &CustomScalarRegistry,
    limits: &ExecutionLimits,
) -> InputCoercionOutput {
    entry::coerce_argument(
        schema, definition, provided, variables, scalars, limits, None,
    )
}

pub use variables::{VariableCoercionOutput, coerce_variable_values, coerce_variables_with_budget};

#[cfg(test)]
#[path = "../tests_input_coercion.rs"]
mod tests;

#[cfg(test)]
mod tests_limits;
#[cfg(test)]
mod tests_plans;
#[cfg(test)]
mod tests_scalars;

#[cfg(test)]
mod tests_budget;
