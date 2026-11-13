//! Validate defaults with registered scalar coercion before startup.

use chigiri_core::ExecutionLimits;
use chigiri_core::executable::{
    CoercedVariables, CustomScalarRegistry, InputCoercionBudget, coerce_literal_with_budget,
};
use chigiri_core::schema::SemanticSchema;

use crate::SchemaBuildError;

pub(super) const COERCION_NODES: usize = 100_000;
pub(super) const COERCION_BYTES: usize = 16 * 1024 * 1024;

pub(super) fn validate(
    schema: &SemanticSchema,
    scalars: &CustomScalarRegistry,
) -> Result<(), SchemaBuildError> {
    let budget = InputCoercionBudget::new(COERCION_NODES, COERCION_BYTES);
    let variables = CoercedVariables::new();
    let limits = ExecutionLimits::production();
    let fields = schema.types().iter().flat_map(|ty| {
        ty.input_fields()
            .iter()
            .chain(ty.fields().iter().flat_map(|field| field.arguments.iter()))
    });
    let directive_arguments = schema
        .directive_definitions()
        .iter()
        .flat_map(|definition| definition.arguments().iter())
        .filter_map(|argument| argument.source_definition.as_ref());
    for field in fields.chain(directive_arguments) {
        if let Some(default) = &field.default_value {
            let output = coerce_literal_with_budget(
                schema,
                &field.ty,
                default,
                &variables,
                scalars,
                &limits,
                Some(&budget),
            );
            if !output.is_valid() {
                return Err(SchemaBuildError::new(
                    "owned schema default violates scalar or coercion budget",
                ));
            }
        }
    }
    Ok(())
}
