//! Describe model storage shapes and read/write capabilities as distinct types.

use super::declarations::typename;
use super::reference;
use super::text::Text;
use super::{ScriptError, names};
use crate::api::context as contract;
use crate::api::model_values::Shape;
use crate::ir::{MockIr, Model};

pub(super) fn generate(out: &mut Text, ir: &MockIr) -> Result<(), ScriptError> {
    out.add("type ChigiriBusinessFailure = { code: string, message: string? }\n")?;
    for (id, model) in ir.models().iter().enumerate() {
        let ty = model.ty;
        out.format(format_args!(
            "type {} = {} & {{\n",
            names::ty(ir, ty, "Record"),
            names::ty(ir, ty, "Stored")
        ))?;
        record_extensions(out, ir, model)?;
        out.add("}\n")?;
        out.format(format_args!(
            "type {} = {} & {{\n",
            names::ty(ir, ty, "ConcreteRecord"),
            names::ty(ir, ty, "Record")
        ))?;
        typename(
            out,
            &ir.graph().types()[ty.0].name,
            !Shape::Record.policy().required_model_typename,
        )?;
        out.add("}\n")?;
        for (prefix, shape) in [
            ("CreateInput", Shape::Create),
            ("UpdateInput", Shape::Update),
        ] {
            out.format(format_args!("type {} = {{\n", names::ty(ir, ty, prefix)))?;
            slots(out, ir, model, shape)?;
            out.add("}\n")?;
        }
        super::query::generate(out, ir, model)?;
        super::methods::generate(out, ir, id, ty)?;
    }
    for (context, prefix) in [
        ("ChigiriReadContext", "ReadModel"),
        ("ChigiriWriteContext", "WriteModel"),
    ] {
        let scope = if prefix == "WriteModel" {
            contract::MUTATION
        } else {
            contract::READ
        };
        out.format(format_args!("type {context} = {{\n"))?;
        out.key(contract::SCOPE)?;
        out.quoted(scope)?;
        out.add(",\n")?;
        common_context(out, ir, prefix)?;
        if prefix == "WriteModel" {
            super::subscription::declarations::write_privilege(out, ir)?;
        }
        out.add("}\n")?;
    }
    Ok(())
}

pub(super) fn common_context(
    out: &mut Text,
    ir: &MockIr,
    model_prefix: &str,
) -> Result<(), ScriptError> {
    common_context_with_request(out, ir, model_prefix, "ChigiriHookRequest | ChigiriNull")
}

pub(super) fn common_context_with_request(
    out: &mut Text,
    ir: &MockIr,
    model_prefix: &str,
    request: &str,
) -> Result<(), ScriptError> {
    out.key(contract::NULL)?;
    out.add("ChigiriNull,\n")?;
    out.key(crate::api::hooks::STATE)?;
    out.add("ChigiriState,\n")?;
    out.key(contract::REQUEST)?;
    out.add(request)?;
    out.add(",\n")?;
    out.key(contract::FAIL)?;
    out.add("(failure: ChigiriBusinessFailure) -> never,\n")?;
    for constructor in contract::CONSTRUCTORS {
        out.key(constructor.name)?;
        out.add("(")?;
        for index in 0..constructor.arity {
            if index > 0 {
                out.add(", ")?;
            }
            out.add(constructor.shape.luau())?;
        }
        out.format(format_args!(") -> {},\n", constructor.shape.luau()))?;
    }
    out.key(contract::MODELS)?;
    out.add("{\n")?;
    for model in ir.models() {
        out.key(&ir.graph().types()[model.ty.0].name)?;
        out.format(format_args!("{},\n", names::ty(ir, model.ty, model_prefix)))?;
    }
    out.add("  },\n")
}

fn slots(out: &mut Text, ir: &MockIr, model: &Model, shape: Shape) -> Result<(), ScriptError> {
    let policy = shape.policy();
    for id in model.slots.values() {
        let slot = &ir.slots()[id.0];
        if !policy.include_key && model.immutable_slots.contains(id) {
            continue;
        }
        out.key(&slot.name)?;
        reference::stored_slot(out, ir, &slot.ty)?;
        // Declarations express nullable patches; Rust revalidates non-null storage constraints.
        if policy.extra_null(slot.ty.nullable()) {
            out.add(" | ChigiriNull")?;
        }
        if policy.optional(slot.ty.nullable())
            || (matches!(shape, Shape::Create) && model.auto_id && Some(slot.field) == model.key)
        {
            out.add(" | nil")?;
        }
        out.add(",\n")?;
    }
    Ok(())
}

fn record_extensions(out: &mut Text, ir: &MockIr, model: &Model) -> Result<(), ScriptError> {
    let fields = &ir.graph().types()[model.ty.0].fields;
    for id in model.slots.values() {
        let slot = &ir.slots()[id.0];
        if fields.get(&slot.name).is_some_and(|field| {
            matches!(
                ir.field_plan(*field),
                Some(crate::ir::FieldPlan::Value | crate::ir::FieldPlan::Stored(_))
            )
        }) {
            continue;
        }
        out.key(&slot.name)?;
        reference::stored_slot(out, ir, &slot.ty)?;
        out.add(",\n")?;
    }
    Ok(())
}
