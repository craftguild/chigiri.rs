//! Validate coerced arguments for every field plan under one shared budget and policy.

use chigiri::core::Object;

use crate::ir::{FieldId, MockIr};
use crate::value::{ValueBudget, ValueError, ValueValidator};

pub(crate) fn validate(
    ir: &MockIr,
    id: FieldId,
    arguments: &Object,
    budget: &mut ValueBudget,
) -> Result<(), ValueError> {
    let field = ir
        .graph()
        .fields()
        .get(id.0)
        .ok_or(ValueError("unknown argument field"))?;
    budget.container(1)?;
    for (name, value) in arguments {
        budget.key(name)?;
        let id = field
            .argument_names
            .get(name)
            .ok_or(ValueError("unknown argument"))?;
        ValueValidator::new(ir, budget).input(*id, value)?;
    }
    for id in &field.arguments {
        let input = &ir.graph().inputs()[id.0];
        if (!input.ty.nullable() || input.default.is_some()) && arguments.get(&input.name).is_none()
        {
            return Err(ValueError("coerced argument missing"));
        }
    }
    Ok(())
}
