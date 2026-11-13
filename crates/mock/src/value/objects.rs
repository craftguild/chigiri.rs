//! Validate exact field sets for input presence, OneOf inputs, and stored DTOs.

use chigiri::core::Value;

use super::{Expected, ValueError, ValueValidator};
use crate::ir::{FieldPlan, TypeId, TypeNode};

impl ValueValidator<'_> {
    pub(super) fn input_object(
        &mut self,
        node: &TypeNode,
        value: &Value,
        depth: usize,
    ) -> Result<(), ValueError> {
        let Value::Object(object) = value else {
            return Err(ValueError("input object required"));
        };
        if node.one_of && (object.len() != 1 || object.iter().any(|(_, value)| value.is_null())) {
            return Err(ValueError("OneOf requires one non-null field"));
        }
        for (name, value) in object {
            self.budget.key(name)?;
            let input = node
                .inputs
                .get(name)
                .ok_or(ValueError("unknown input field"))?;
            let input = &self.ir.graph().inputs()[input.0];
            self.typed(
                Expected::new(&input.ty, &input.constraints),
                value,
                depth + 1,
            )?;
        }
        for input in &node.required_inputs {
            let input = &self.ir.graph().inputs()[input.0];
            // After coercion, a field with a declared default must have a concrete value.
            if !object.contains_key(&input.name) {
                return Err(ValueError("coerced input field missing"));
            }
        }
        Ok(())
    }

    pub(super) fn stored_abstract(
        &mut self,
        expected: TypeId,
        value: &Value,
        depth: usize,
    ) -> Result<(), ValueError> {
        let Value::Object(object) = value else {
            return Err(ValueError("stored abstract object required"));
        };
        let concrete = object
            .get(crate::api::model_values::TYPENAME)
            .and_then(Value::as_str)
            .and_then(|name| self.ir.graph().type_id(name))
            .ok_or(ValueError(
                "stored abstract value requires concrete typename",
            ))?;
        if self.ir.graph().types()[expected.0]
            .possible_types
            .binary_search(&concrete)
            .is_err()
        {
            return Err(ValueError("stored type is not a possible member"));
        }
        self.stored_object(concrete, value, depth)
    }

    pub(super) fn stored_object(
        &mut self,
        ty: TypeId,
        value: &Value,
        depth: usize,
    ) -> Result<(), ValueError> {
        let node = &self.ir.graph().types()[ty.0];
        let Value::Object(object) = value else {
            return Err(ValueError("stored DTO object required"));
        };
        for (name, value) in object {
            self.budget.key(name)?;
            if name == crate::api::model_values::TYPENAME {
                self.budget.node(value, depth + 1)?;
                if value.as_str() != Some(node.name.as_ref()) {
                    return Err(ValueError("stored concrete typename mismatch"));
                }
                continue;
            }
            let field = node
                .fields
                .get(name)
                .ok_or(ValueError("unknown DTO field"))?;
            if !matches!(
                self.ir.field_plan(*field),
                Some(FieldPlan::Value | FieldPlan::Stored(_))
            ) {
                return Err(ValueError("computed DTO field cannot be stored"));
            }
            let field = &self.ir.graph().fields()[field.0];
            self.typed(
                Expected::new(&field.ty, &field.constraints),
                value,
                depth + 1,
            )?;
        }
        for field in self.ir.required_value_fields(ty) {
            if !object.contains_key(&self.ir.graph().fields()[field.0].name) {
                return Err(ValueError("required DTO field missing"));
            }
        }
        Ok(())
    }
}
