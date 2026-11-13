//! Build query operands without applying storage-only bounds or key-generation rules.

use super::{Atom, Compiler, Operand};
use crate::api::query::{IN_ITEMS, OPERAND_BYTES, OperandType, ScalarKind};
use crate::value::{ValueError, exact_number};
use chigiri::core::Value;

impl Compiler<'_, '_> {
    pub(super) fn operand<'a>(
        &mut self,
        value: &'a Value,
        kind: ScalarKind,
        shape: OperandType,
        depth: usize,
    ) -> Result<Operand<'a>, ValueError> {
        match shape {
            OperandType::Scalar => self.atom(value, kind, depth).map(Operand::Scalar),
            OperandType::Boolean => self
                .atom(value, ScalarKind::Boolean, depth)
                .map(Operand::Scalar),
            OperandType::List => {
                let Value::List(values) = value else {
                    return Err(ValueError("in array required"));
                };
                self.budget.container(depth)?;
                if values.len() > IN_ITEMS {
                    return Err(ValueError("in candidate limit"));
                }
                values
                    .iter()
                    .map(|value| self.atom(value, kind, depth + 1))
                    .collect::<Result<Vec<_>, _>>()
                    .map(Operand::List)
            }
        }
    }

    fn atom<'a>(
        &mut self,
        value: &'a Value,
        kind: ScalarKind,
        depth: usize,
    ) -> Result<Atom<'a>, ValueError> {
        self.budget.node(value, depth)?;
        self.work.charge(1)?;
        match (kind, value) {
            (ScalarKind::String | ScalarKind::Id | ScalarKind::Enum(_), Value::String(value)) => {
                self.work.charge(value.len())?;
                if value.len() > OPERAND_BYTES {
                    return Err(ValueError("operand string limit"));
                }
                if kind == ScalarKind::Id
                    && (value.is_empty() || value.len() > crate::limits::ID_BYTES)
                {
                    return Err(ValueError("invalid ID operand"));
                }
                if let ScalarKind::Enum(ty) = kind
                    && !self.ir.graph().types()[ty.0]
                        .enum_index
                        .contains(value.as_str())
                {
                    return Err(ValueError("unknown enum operand"));
                }
                Ok(Atom::Text(value))
            }
            (ScalarKind::Boolean, Value::Boolean(value)) => Ok(Atom::Boolean(*value)),
            (kind, value) if kind.numeric() => {
                if kind == ScalarKind::Int
                    && !matches!(value, Value::Int(value) if i32::try_from(*value).is_ok())
                {
                    return Err(ValueError("invalid Int operand"));
                }
                let number = exact_number(value)?;
                if kind == ScalarKind::Integer && number.fract() != 0.0 {
                    return Err(ValueError("integer operand required"));
                }
                Ok(Atom::Number(number))
            }
            _ => Err(ValueError("invalid or null operand")),
        }
    }
}
