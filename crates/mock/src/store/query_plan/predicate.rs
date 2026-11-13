//! Compile every predicate independently, counting group nodes even when evaluation will short-circuit.

use super::{Compiler, Predicate, text};
use crate::api::query::{OPERATORS, PREDICATE_DEPTH, PREDICATE_NODES};
use crate::api::shape::{self, Group};
use crate::value::ValueError;
use chigiri::core::Value;

impl Compiler<'_, '_> {
    pub(super) fn predicate<'a>(
        &mut self,
        value: &'a Value,
        depth: usize,
        value_depth: usize,
    ) -> Result<Predicate<'a>, ValueError> {
        self.nodes += 1;
        self.work.charge(1)?;
        if depth > PREDICATE_DEPTH || self.nodes > PREDICATE_NODES {
            return Err(ValueError("predicate resource limit"));
        }
        let Value::Object(object) = value else {
            return Err(ValueError("predicate object required"));
        };
        self.object(object, value_depth)?;
        if let Some((group, shape)) = shape::GROUPS
            .iter()
            .find(|(_, shape)| shape.accepts(object))
        {
            let children = object
                .get(shape.fields[0].name)
                .ok_or(ValueError("predicate group required"))?;
            let Value::List(children) = children else {
                return Err(ValueError("predicate array required"));
            };
            self.budget.container(value_depth + 1)?;
            self.budget.items(children.len())?;
            let children = children
                .iter()
                .map(|child| self.predicate(child, depth + 1, value_depth + 2))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(match group {
                Group::All => Predicate::All(children),
                Group::Any => Predicate::Any(children),
            });
        }
        if !shape::LEAF.accepts(object) {
            return Err(ValueError("invalid predicate keys"));
        }
        let field = object
            .get(shape::FIELD.name)
            .ok_or(ValueError("predicate field required"))?;
        let op = object
            .get(shape::OP.name)
            .ok_or(ValueError("predicate op required"))?;
        let value = object
            .get(shape::VALUE.name)
            .ok_or(ValueError("predicate value required"))?;
        self.budget.node(field, value_depth + 1)?;
        self.budget.node(op, value_depth + 1)?;
        let (slot, kind) = self.slot(field)?;
        let spec = OPERATORS
            .iter()
            .find(|spec| spec.name == text(op).unwrap_or(""))
            .ok_or(ValueError("unknown predicate operator"))?;
        if !spec.eligibility.allows(kind) {
            return Err(ValueError("operator is not eligible for slot"));
        }
        let value = self.operand(value, kind, spec.operand, value_depth + 1)?;
        Ok(Predicate::Leaf {
            slot,
            op: spec.operator,
            value,
        })
    }
}
