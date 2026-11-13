//! Generate type selectors with destination-owned source and resolver contracts.

use super::super::ScriptError;
use super::super::names;
use super::super::reference::{self, Mode};
use super::super::text::Text;
use crate::api::{context, subscriptions as contract};
use crate::ir::{MockIr, ScriptId, ScriptRole};

/// Declare nominal source classes only in the checker definition environment.
/// A table or another field's source cannot satisfy these opaque host types.
pub(in crate::luau) fn host(out: &mut Text, ir: &MockIr) -> Result<(), ScriptError> {
    for (id, plan) in ir.scripts().iter().enumerate() {
        if plan.role.is_subscription_root() {
            out.format(format_args!(
                "declare extern type {} with\nend\n",
                names::resolver(ir, ScriptId(id), "Source")
            ))?;
        }
    }
    Ok(())
}

pub(in crate::luau) fn shared(out: &mut Text, ir: &MockIr) -> Result<(), ScriptError> {
    if ir.graph().subscription().is_none() {
        return Ok(());
    }
    out.add("type ChigiriSubscriptionOverflow = ")?;
    out.quoted(contract::BACKPRESSURE)?;
    out.add(" | ")?;
    out.quoted(contract::DROP_OLDEST)?;
    out.add("\ntype ChigiriSubscriptionPubSubInput = {\n")?;
    out.key(contract::TOPIC)?;
    out.add("string,\n")?;
    out.key(contract::OVERFLOW)?;
    out.add("ChigiriSubscriptionOverflow?,\n}\n")?;
    for event in crate::events::payload::event_types(ir) {
        out.format(format_args!(
            "type {} = {{\n",
            names::ty(ir, event.named, "EventPublishInput")
        ))?;
        out.key(contract::TOPIC)?;
        out.add("string,\n")?;
        out.key(contract::PAYLOAD)?;
        publication_payload(out, ir, &event.fields)?;
        out.add(",\n}\n")?;
    }
    root_parent(out, ir)
}

pub(in crate::luau) fn publication_payload(
    out: &mut Text,
    ir: &MockIr,
    fields: &[crate::ir::FieldId],
) -> Result<(), ScriptError> {
    let mut shapes = Vec::new();
    for field in fields {
        let ty = &ir.graph().fields()[field.0].ty;
        if shapes.contains(&ty) {
            continue;
        }
        if !shapes.is_empty() {
            out.add(" | ")?;
        }
        reference::output_alternative(out, ir, ty)?;
        shapes.push(ty);
    }
    Ok(())
}

fn root_parent(out: &mut Text, ir: &MockIr) -> Result<(), ScriptError> {
    let root = ir.graph().subscription().unwrap();
    let node = &ir.graph().types()[root.0];
    out.format(format_args!("type {} = {{\n", names::ty(ir, root, "")))?;
    out.key(crate::api::model_values::TYPENAME)?;
    out.quoted(&node.name)?;
    out.add("?,\n")?;
    for field in node.fields.values() {
        let field = &ir.graph().fields()[field.0];
        out.key(&field.name)?;
        reference::reference(out, ir, &field.ty, Mode::Output)?;
        if field.ty.nullable() {
            out.add("?")?;
        }
        out.add(",\n")?;
    }
    out.add("}\n")
}

pub(in crate::luau) fn setup_context(out: &mut Text, ir: &MockIr) -> Result<(), ScriptError> {
    for (id, plan) in ir.scripts().iter().enumerate() {
        if !plan.role.is_subscription_root() {
            continue;
        }
        let id = ScriptId(id);
        out.format(format_args!(
            "type {} = {{\n",
            names::resolver(ir, id, "SetupContext")
        ))?;
        out.key(context::SCOPE)?;
        out.quoted(contract::SETUP_SCOPE)?;
        out.add(",\n")?;
        super::super::models::common_context(out, ir, "ReadModel")?;
        source_privilege(out, ir, id, "ChigiriSubscriptionPubSubInput")?;
        out.add("}\n")?;
    }
    Ok(())
}

pub(in crate::luau) fn source_privilege(
    out: &mut Text,
    ir: &MockIr,
    id: ScriptId,
    pubsub_input: &str,
) -> Result<(), ScriptError> {
    let field = ir.scripts()[id.0].field;
    let type_name = crate::events::payload::field_type_name(ir, field);
    out.key(contract::PUBSUB)?;
    out.add("{\n")?;
    out.key(contract::SUBSCRIBE)?;
    out.add("{\n")?;
    out.key(type_name)?;
    out.format(format_args!(
        "(input: {pubsub_input}) -> {},\n}},\n}},\n",
        names::resolver(ir, id, "Source")
    ))?;
    out.key(contract::GENERATOR)?;
    out.add("{\n")?;
    out.key(contract::SEQUENCE)?;
    out.add("{\n")?;
    out.key(type_name)?;
    out.format(format_args!(
        "(input: {}) -> {},\n}},\n}},\n",
        names::resolver(ir, id, "SequenceInput"),
        names::resolver(ir, id, "Source")
    ))
}

pub(in crate::luau) fn write_privilege(out: &mut Text, ir: &MockIr) -> Result<(), ScriptError> {
    let types = crate::events::payload::event_types(ir);
    if types.is_empty() {
        return Ok(());
    }
    out.key(contract::PUBSUB)?;
    out.add("{\n")?;
    out.key(contract::PUBLISH)?;
    out.add("{\n")?;
    for event in types {
        out.key(&ir.graph().types()[event.named.0].name)?;
        out.format(format_args!(
            "(input: {}) -> (),\n",
            names::ty(ir, event.named, "EventPublishInput")
        ))?;
    }
    out.add("},\n},\n")
}

pub(in crate::luau) fn coordinate(
    out: &mut Text,
    ir: &MockIr,
    id: ScriptId,
) -> Result<(), ScriptError> {
    let plan = &ir.scripts()[id.0];
    let ScriptRole::SubscriptionRoot { root_as_nested } = plan.role else {
        return Err(ScriptError::new(
            "contract",
            "subscription declaration requires subscription role",
        ));
    };
    let field = &ir.graph().fields()[plan.field.0];
    sequence_input(out, ir, id)?;
    if root_as_nested {
        out.format(format_args!(
            "type {} = ",
            names::resolver(ir, id, "Result")
        ))?;
        reference::reference(out, ir, &field.ty, Mode::Output)?;
        out.add("\n")?;
    }
    out.format(format_args!(
        "type {} = {{\n    subscribe: (",
        names::resolver(ir, id, "Resolver")
    ))?;
    subscribe_parameters(out, ir, id)?;
    out.format(format_args!(
        ") -> {},\n",
        names::resolver(ir, id, "Source")
    ))?;
    if root_as_nested {
        out.add("    resolve: (")?;
        resolve_parameters(out, ir, id)?;
        out.format(format_args!(
            ") -> {},\n",
            names::resolver(ir, id, "Result")
        ))?;
    }
    out.add("}\n")
}

pub(in crate::luau) fn sequence_input(
    out: &mut Text,
    ir: &MockIr,
    id: ScriptId,
) -> Result<(), ScriptError> {
    let field = &ir.graph().fields()[ir.scripts()[id.0].field.0];
    out.format(format_args!(
        "type {} = {{\n",
        names::resolver(ir, id, "SequenceInput")
    ))?;
    out.key(contract::VALUES)?;
    out.add("{")?;
    reference::reference(out, ir, &field.ty, Mode::Output)?;
    out.add("},\n")?;
    out.key(contract::INTERVAL_MS)?;
    out.add("number,\n")?;
    out.key(contract::LOOP)?;
    out.add("boolean?,\n")?;
    out.key(contract::OVERFLOW)?;
    out.add("ChigiriSubscriptionOverflow?,\n}\n")
}

pub(super) fn subscribe_parameters(
    out: &mut Text,
    ir: &MockIr,
    id: ScriptId,
) -> Result<(), ScriptError> {
    out.format(format_args!(
        "ctx: {}, args: {}",
        names::resolver(ir, id, "SetupContext"),
        names::resolver(ir, id, "Args")
    ))
}

pub(super) fn resolve_parameters(
    out: &mut Text,
    ir: &MockIr,
    id: ScriptId,
) -> Result<(), ScriptError> {
    let plan = &ir.scripts()[id.0];
    let field = &ir.graph().fields()[plan.field.0];
    out.format(format_args!(
        "ctx: ChigiriReadContext, args: {}, parent: {}",
        names::resolver(ir, id, "Args"),
        names::ty(ir, field.parent, "")
    ))
}
