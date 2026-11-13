//! Validate borrowed coerced inputs and stored values against IR types, constraints, and shared resource limits.

mod budget;
pub(crate) mod conditions;
pub(crate) mod decimal;
mod defaults;
mod equality;
mod formats;
pub(crate) mod json;
pub(crate) mod numbers;
mod objects;
mod registry;
mod scalar;
pub(crate) mod serialize;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_conditions;
#[cfg(test)]
mod tests_decimal;
#[cfg(test)]
mod tests_defaults;
#[cfg(test)]
mod tests_formats;
#[cfg(test)]
mod tests_limits;
#[cfg(test)]
mod tests_scope;
pub(crate) mod work;

use std::fmt;

use chigiri::core::Value;
use chigiri::core::schema::TypeKind;

use crate::ir::{Constraints, FieldPlan, InputId, MockIr, SlotId, TypeId, TypeRef, Wrapper};

pub use budget::{MAX_BYTES, MAX_DEPTH, MAX_ITEMS, MAX_NODES, MAX_STRING_BYTES, ValueBudget};
pub(crate) use conditions::constraints;
pub(crate) use defaults::validate as validate_defaults;
pub(crate) use equality::equal;
pub use numbers::exact_number;
pub use registry::{public_scalar_registry, scalar_registry};

/// Error returned while validating or converting a value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValueError(pub &'static str);

impl fmt::Display for ValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for ValueError {}

/// Value validator that charges multiple conversions to one shared budget.
pub struct ValueValidator<'a> {
    ir: &'a MockIr,
    budget: &'a mut ValueBudget,
}

#[derive(Clone, Copy)]
struct Expected<'a> {
    named: TypeId,
    wrappers: &'a [Wrapper],
    constraints: Option<&'a Constraints>,
    identity: bool,
    outer: bool,
}

impl<'a> Expected<'a> {
    fn new(ty: &'a TypeRef, constraints: &'a Constraints) -> Self {
        Self {
            named: ty.named,
            wrappers: &ty.wrappers,
            constraints: Some(constraints),
            identity: false,
            outer: true,
        }
    }
}

impl<'a> ValueValidator<'a> {
    /// Validate against one schema and a caller-owned shared budget.
    pub fn new(ir: &'a MockIr, budget: &'a mut ValueBudget) -> Self {
        Self { ir, budget }
    }

    /// Validate an input or argument after defaults and GraphQL coercion.
    pub fn input(&mut self, id: InputId, value: &Value) -> Result<(), ValueError> {
        let input = self
            .ir
            .graph()
            .inputs()
            .get(id.0)
            .ok_or(ValueError("unknown input"))?;
        self.typed(Expected::new(&input.ty, &input.constraints), value, 1)
    }

    /// Validate one stored slot value.
    pub fn slot(&mut self, id: SlotId, value: &Value) -> Result<(), ValueError> {
        self.slot_at_depth(id, value, 1)
    }

    pub(crate) fn slot_at_depth(
        &mut self,
        id: SlotId,
        value: &Value,
        depth: usize,
    ) -> Result<(), ValueError> {
        let slot = self
            .ir
            .slots()
            .get(id.0)
            .ok_or(ValueError("unknown slot"))?;
        let mut expected = Expected::new(&slot.ty, &slot.constraints);
        expected.identity = self.ir.models()[slot.model.0].immutable_slots.contains(&id)
            || matches!(self.ir.field_plan(slot.field), Some(FieldPlan::Owner(_)));
        self.typed(expected, value, depth)
    }

    /// Validate a custom-scalar representation; the caller owns wrapper and nullability checks.
    pub fn scalar(&mut self, ty: TypeId, value: &Value) -> Result<(), ValueError> {
        let node = self
            .ir
            .graph()
            .types()
            .get(ty.0)
            .ok_or(ValueError("unknown scalar"))?;
        let spec = node
            .scalar
            .as_ref()
            .ok_or(ValueError("custom scalar required"))?;
        self.budget.node(value, 1)?;
        if value.is_null() {
            return Ok(());
        }
        scalar::custom(spec, value, 1, self.budget)
    }

    fn typed(
        &mut self,
        mut expected: Expected<'_>,
        value: &Value,
        depth: usize,
    ) -> Result<(), ValueError> {
        self.budget.node(value, depth)?;
        if expected.wrappers.first() == Some(&Wrapper::NonNull) {
            if value.is_null() {
                return Err(ValueError("null for non-null value"));
            }
            expected.wrappers = &expected.wrappers[1..];
        } else if value.is_null() {
            return Ok(());
        }
        if expected.wrappers.first() == Some(&Wrapper::List) {
            let Value::List(items) = value else {
                return Err(ValueError("list required"));
            };
            self.budget.items(items.len())?;
            if expected.outer
                && let Some(bounds) = expected.constraints
            {
                conditions::list(bounds, items.len(), self.budget)?;
            }
            expected.wrappers = &expected.wrappers[1..];
            expected.outer = false;
            for item in items {
                self.typed(expected, item, depth + 1)?;
            }
            return Ok(());
        }
        if let Some(bounds) = expected.constraints {
            if expected.outer {
                conditions::constraints(bounds, value, self.budget)?;
            } else {
                conditions::leaf(bounds, value, self.budget)?;
            }
        }
        let node = &self.ir.graph().types()[expected.named.0];
        match node.kind {
            TypeKind::Scalar | TypeKind::Enum => {
                self.leaf(expected.named, value, depth, expected.identity)
            }
            TypeKind::InputObject => self.input_object(node, value, depth),
            TypeKind::Object => self.stored_object(expected.named, value, depth),
            TypeKind::Interface | TypeKind::Union => {
                self.stored_abstract(expected.named, value, depth)
            }
        }
    }

    /// The caller charges the root node; this method charges only JSON nested inside the scalar.
    pub(crate) fn leaf(
        &mut self,
        ty: TypeId,
        value: &Value,
        depth: usize,
        identity: bool,
    ) -> Result<(), ValueError> {
        let node = &self.ir.graph().types()[ty.0];
        if node.kind == TypeKind::Enum {
            let Value::String(name) = value else {
                return Err(ValueError("enum string required"));
            };
            if name.len() > MAX_STRING_BYTES
                || (identity && name.len() > crate::limits::ID_BYTES)
                || !node.enum_index.contains(name.as_str())
            {
                return Err(ValueError("unknown or oversized enum value"));
            }
            return Ok(());
        }
        if let Some(spec) = &node.scalar {
            scalar::custom(spec, value, depth, self.budget)
        } else {
            scalar::builtin(&node.name, value, identity)
        }
    }
}
