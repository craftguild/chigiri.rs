//! Applies raw request values and defaults to selected-operation variable definitions.

use std::collections::BTreeMap;

use crate::language::ast::{TypeRefKind, VariableDefinition};
use crate::schema::SemanticSchema;
use crate::value::{drop_iterative, within_depth_limit};
use crate::{DiagnosticBag, ExecutionLimits, Object};

use super::engine::{Engine, Source};
use super::{
    CoercedInput, CoercedVariables, CustomScalarRegistry, INPUT_NESTING_LIMIT,
    INPUT_REQUIRED_VALUE_MISSING,
};

/// Per-request variable-coercion result and bounded diagnostics.
#[derive(Clone, Debug)]
pub struct VariableCoercionOutput {
    /// Return presence information only when every variable was coerced successfully.
    pub variables: Option<CoercedVariables>,
    /// Client-safe diagnostics for variable definitions.
    pub diagnostics: DiagnosticBag,
    diagnostics_truncated: bool,
}

impl VariableCoercionOutput {
    /// Returns the raw variable value for a complete definition.
    pub const fn is_valid(&self) -> bool {
        self.variables.is_some()
    }

    /// Returns whether coercion reached the diagnostic limit.
    pub const fn diagnostics_were_truncated(&self) -> bool {
        self.diagnostics_truncated
    }
}

/// Apply selected-operation variable definitions to the raw request map.
///
/// The raw key that is not declared is ignored by the GraphQL rule.
/// The result is not cached with validated documents.
pub fn coerce_variable_values(
    schema: &SemanticSchema,
    definitions: &[VariableDefinition],
    raw: Object,
    scalars: &CustomScalarRegistry,
    limits: &ExecutionLimits,
) -> VariableCoercionOutput {
    coerce_variables_with_budget(schema, definitions, raw, scalars, limits, None)
}

/// Coerces variables and defaults using a shared request budget.
pub fn coerce_variables_with_budget(
    schema: &SemanticSchema,
    definitions: &[VariableDefinition],
    raw: Object,
    scalars: &CustomScalarRegistry,
    limits: &ExecutionLimits,
    budget: Option<&super::InputCoercionBudget>,
) -> VariableCoercionOutput {
    let mut raw = raw.into_iter().collect::<BTreeMap<_, _>>();
    let mut engine = Engine::new(schema, scalars, None, limits).with_budget(budget);
    let mut variables = CoercedVariables::new();
    for definition in definitions {
        let name = &definition.variable.value;
        engine.set_fallback_span(Some(definition.variable.span));
        engine.push_path(format!("${name}"));
        let value = match raw.remove(name) {
            Some(raw_value) if within_depth_limit(&raw_value, limits.max_parser_nesting) => {
                engine.coerce(Source::Raw(raw_value), &definition.ty, 0)
            }
            Some(raw_value) => {
                engine.report(
                    INPUT_NESTING_LIMIT,
                    format!("Variable `${name}` exceeds the input nesting limit"),
                    Some(definition.variable.span),
                );
                drop_iterative(raw_value);
                None
            }
            None => match &definition.default_value {
                Some(default) => engine.coerce(Source::Literal(default), &definition.ty, 0),
                None if matches!(definition.ty.kind, TypeRefKind::NonNull(_)) => {
                    engine.report(
                        INPUT_REQUIRED_VALUE_MISSING,
                        format!("Required variable `${name}` was not provided"),
                        Some(definition.variable.span),
                    );
                    None
                }
                None => Some(CoercedInput::Undefined),
            },
        };
        if let Some(value) = value {
            variables.insert(name.clone(), value);
        }
        engine.pop_path();
        if !engine.can_continue() {
            break;
        }
    }
    for (_, unused) in raw {
        drop_iterative(unused);
    }
    let state = engine.finish(Some(CoercedInput::Undefined));
    VariableCoercionOutput {
        variables: state.is_valid().then_some(variables),
        diagnostics_truncated: state.diagnostics_were_truncated(),
        diagnostics: state.diagnostics,
    }
}
