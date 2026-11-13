//! Derive each stored slot's operator and operand types from the shared closed query contract.

use super::{
    ScriptError, names,
    reference::{self, Mode},
    text::Text,
};
use crate::api::query::{DIRECTIONS, OPERATORS, OperandType, scalar};
use crate::api::shape;
use crate::ir::{MockIr, Model, TypeRef, Wrapper};
use std::sync::Arc;

pub(super) fn generate(out: &mut Text, ir: &MockIr, model: &Model) -> Result<(), ScriptError> {
    let predicate = names::ty(ir, model.ty, "Where");
    out.format(format_args!("type {predicate} = "))?;
    for (index, (_, group)) in shape::GROUPS.iter().enumerate() {
        if index > 0 {
            out.add(" | ")?;
        }
        out.add("{ ")?;
        out.key(group.fields[0].name)?;
        out.format(format_args!("{{{predicate}}} }}"))?;
    }
    for slot in model.slots.values() {
        let slot = &ir.slots()[slot.0];
        let Some(kind) = scalar(ir, slot) else {
            continue;
        };
        let required = TypeRef {
            named: slot.ty.named,
            wrappers: Arc::from([Wrapper::NonNull]),
        };
        for operator in OPERATORS {
            if !operator.eligibility.allows(kind) {
                continue;
            }
            out.add(" | { ")?;
            out.key(shape::FIELD.name)?;
            out.quoted(&slot.name)?;
            out.add(", ")?;
            out.key(shape::OP.name)?;
            out.quoted(operator.name)?;
            out.add(", ")?;
            out.key(shape::VALUE.name)?;
            match operator.operand {
                OperandType::Boolean => out.add("boolean")?,
                OperandType::Scalar => reference::reference(out, ir, &required, Mode::Stored)?,
                OperandType::List => {
                    out.add("{")?;
                    reference::reference(out, ir, &required, Mode::Stored)?;
                    out.add("}")?;
                }
            }
            out.add(" }")?;
        }
    }
    out.format(format_args!(
        "\ntype {} = {{ ",
        names::ty(ir, model.ty, "OrderKey")
    ))?;
    out.key(shape::FIELD.name)?;
    let mut separator = "";
    for slot in model.slots.values() {
        let slot = &ir.slots()[slot.0];
        if scalar(ir, slot).is_some() {
            out.add(separator)?;
            out.quoted(&slot.name)?;
            separator = " | ";
        }
    }
    // A model with no sortable scalar slot still needs a well-formed but uninhabited order-key type.
    if separator.is_empty() {
        out.add("never")?;
    }
    out.add(", ")?;
    out.key(shape::DIRECTION.name)?;
    for (index, (direction, _)) in DIRECTIONS.iter().enumerate() {
        if index > 0 {
            out.add(" | ")?;
        }
        out.quoted(direction)?;
    }
    out.add(" }\n")
}
