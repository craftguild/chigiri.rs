//! Entry point for single-value coercion and argument-default application.

use crate::language::ast::{InputValueDefinition, TypeRef, TypeRefKind, ValueNode};
use crate::schema::SemanticSchema;
use crate::value::{drop_iterative, within_depth_limit};
use crate::{ExecutionLimits, Value};

use super::engine::{Engine, Source};
use super::{
    CoercedInput, CoercedVariables, CustomScalarRegistry, INPUT_NESTING_LIMIT,
    INPUT_REQUIRED_VALUE_MISSING, InputCoercionOutput,
};

pub(super) fn coerce_raw(
    schema: &SemanticSchema,
    expected: &TypeRef,
    raw: Value,
    scalars: &CustomScalarRegistry,
    limits: &ExecutionLimits,
    budget: Option<&super::InputCoercionBudget>,
) -> InputCoercionOutput {
    let mut engine = Engine::new(schema, scalars, None, limits).with_budget(budget);
    if !within_depth_limit(&raw, limits.max_parser_nesting) {
        engine.report(
            INPUT_NESTING_LIMIT,
            "Input value nesting limit exceeded",
            None,
        );
        drop_iterative(raw);
        return engine.finish(None);
    }
    let value = engine.coerce(Source::Raw(raw), expected, 0);
    engine.finish(value)
}

pub(super) fn coerce_literal(
    schema: &SemanticSchema,
    expected: &TypeRef,
    literal: &ValueNode,
    variables: &CoercedVariables,
    scalars: &CustomScalarRegistry,
    limits: &ExecutionLimits,
    budget: Option<&super::InputCoercionBudget>,
) -> InputCoercionOutput {
    let mut engine = Engine::new(schema, scalars, Some(variables), limits).with_budget(budget);
    let value = engine.coerce(Source::Literal(literal), expected, 0);
    engine.finish(value)
}

pub(super) fn coerce_argument(
    schema: &SemanticSchema,
    definition: &InputValueDefinition,
    provided: Option<&ValueNode>,
    variables: &CoercedVariables,
    scalars: &CustomScalarRegistry,
    limits: &ExecutionLimits,
    budget: Option<&super::InputCoercionBudget>,
) -> InputCoercionOutput {
    let mut engine = Engine::new(schema, scalars, Some(variables), limits).with_budget(budget);
    engine.set_fallback_span(Some(definition.span));
    if !engine.reserve(0, definition.name.value.len(), Some(definition.name.span)) {
        return engine.finish(None);
    }
    engine.push_path(definition.name.value.clone());
    let mut value =
        provided.and_then(|value| engine.coerce(Source::Literal(value), &definition.ty, 0));
    if provided.is_none() || matches!(value, Some(CoercedInput::Undefined)) {
        value = definition
            .default_value
            .as_ref()
            .and_then(|default| engine.coerce(Source::Literal(default), &definition.ty, 0));
        if definition.default_value.is_none() {
            value = Some(CoercedInput::Undefined);
        }
    }
    if matches!(value, Some(CoercedInput::Undefined))
        && matches!(definition.ty.kind, TypeRefKind::NonNull(_))
    {
        engine.report(
            INPUT_REQUIRED_VALUE_MISSING,
            format!(
                "Required input `{}` was not provided",
                definition.name.value
            ),
            Some(definition.span),
        );
    }
    engine.pop_path();
    engine.finish(value)
}
