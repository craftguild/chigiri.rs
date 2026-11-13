//! Budget-aware entry points for the shared coercion engine.

use crate::language::ast::{InputValueDefinition, TypeRef, ValueNode};
use crate::schema::SemanticSchema;
use crate::{ExecutionLimits, Value};

use super::{
    CoercedVariables, CustomScalarRegistry, InputCoercionBudget, InputCoercionOutput, entry,
};

/// Coerce raw input while charging traversal and cloning to the shared budget.
pub fn coerce_raw_with_budget(
    schema: &SemanticSchema,
    expected: &TypeRef,
    raw: Value,
    scalars: &CustomScalarRegistry,
    limits: &ExecutionLimits,
    budget: Option<&InputCoercionBudget>,
) -> InputCoercionOutput {
    entry::coerce_raw(schema, expected, raw, scalars, limits, budget)
}

/// Coerce a literal under a shared budget.
pub fn coerce_literal_with_budget(
    schema: &SemanticSchema,
    expected: &TypeRef,
    literal: &ValueNode,
    variables: &CoercedVariables,
    scalars: &CustomScalarRegistry,
    limits: &ExecutionLimits,
    budget: Option<&InputCoercionBudget>,
) -> InputCoercionOutput {
    entry::coerce_literal(
        schema, expected, literal, variables, scalars, limits, budget,
    )
}

/// Resolve argument presence and defaults under a shared budget.
pub fn coerce_argument_with_budget(
    schema: &SemanticSchema,
    definition: &InputValueDefinition,
    provided: Option<&ValueNode>,
    variables: &CoercedVariables,
    scalars: &CustomScalarRegistry,
    limits: &ExecutionLimits,
    budget: Option<&InputCoercionBudget>,
) -> InputCoercionOutput {
    entry::coerce_argument(
        schema, definition, provided, variables, scalars, limits, budget,
    )
}
