//! Coerce every declared default once and validate it against Mock input constraints.

use chigiri::core::ExecutionLimits;
use chigiri::core::executable::{
    CoercedVariables, InputCoercionBudget, coerce_literal_with_budget,
};

use super::{Expected, MAX_BYTES, MAX_NODES, ValueBudget, ValueValidator, scalar_registry};
use crate::ir::{Constraints, InputId, MockIr};
use crate::schema::SchemaError;

pub(crate) fn validate(ir: &MockIr) -> Result<(), SchemaError> {
    let mut budget = ValueBudget::with_work(ir.graph().startup_work().clone());
    let mut validator = ValueValidator::new(ir, &mut budget);
    coerce(ir, &mut validator)
}

fn coerce(ir: &MockIr, validator: &mut ValueValidator<'_>) -> Result<(), SchemaError> {
    let schema = ir.graph().source().semantic();
    let scalars = scalar_registry(ir);
    let variables = CoercedVariables::new();
    let limits = ExecutionLimits::production();
    let budget = InputCoercionBudget::new(MAX_NODES, MAX_BYTES);
    // Match graph input registration order: each type's field arguments, then its input fields.
    let definitions = schema.types().iter().flat_map(|ty| {
        ty.fields()
            .iter()
            .flat_map(|field| &field.arguments)
            .chain(ty.input_fields())
    });
    for (index, definition) in definitions.enumerate() {
        let Some(default) = &definition.default_value else {
            continue;
        };
        let output = coerce_literal_with_budget(
            schema,
            &definition.ty,
            default,
            &variables,
            &scalars,
            &limits,
            Some(&budget),
        );
        let id = InputId(index);
        let value = output
            .value
            .and_then(|value| value.into_value())
            .ok_or_else(|| error(ir, id))?;
        validator.input(id, &value).map_err(|_| error(ir, id))?;
    }
    let directive_arguments = schema
        .directive_definitions()
        .iter()
        .flat_map(|directive| directive.arguments())
        .filter_map(|argument| argument.source_definition.as_ref());
    for definition in directive_arguments {
        let Some(default) = &definition.default_value else {
            continue;
        };
        let invalid = || {
            SchemaError::new(
                "mock default violates value constraints",
                Some(definition.name.span),
            )
        };
        let output = coerce_literal_with_budget(
            schema,
            &definition.ty,
            default,
            &variables,
            &scalars,
            &limits,
            Some(&budget),
        );
        let value = output
            .value
            .and_then(|value| value.into_value())
            .ok_or_else(invalid)?;
        let ty = ir.graph().resolve_ref(&definition.ty)?;
        validator
            .typed(Expected::new(&ty, &Constraints::default()), &value, 1)
            .map_err(|_| invalid())?;
    }
    Ok(())
}

fn error(ir: &MockIr, id: InputId) -> SchemaError {
    SchemaError::new(
        "mock default violates value constraints",
        Some(ir.graph().inputs()[id.0].span),
    )
}
