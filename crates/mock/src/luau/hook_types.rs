//! Generate the six hook-slot contracts from state declarations and shared host definitions.

use crate::api::hook_values::{HookValue as V, LOG_FUNCTION, REJECT_FUNCTION};
use crate::api::hooks as api;
use crate::hooks::{HookSlot, StateContract, StateFields, StateType};

use super::text::Text;
use super::{MAX_BODY, ScriptError};

pub(super) fn generate(out: &mut Text, state: &StateContract) -> Result<(), ScriptError> {
    for (name, patch) in [("ChigiriState", false), ("ChigiriStatePatch", true)] {
        out.format(format_args!("type {name} = "))?;
        object(out, state.fields(), patch)?;
        out.add("\n")?;
    }
    for shape in api::SNAPSHOTS {
        out.format(format_args!("type {} = {{\n", shape.alias))?;
        for (name, ty) in shape.fields {
            out.key(name)?;
            host_value(out, ty)?;
            out.add(",\n")?;
        }
        out.add("}\n")?;
    }
    for function in [&LOG_FUNCTION, &REJECT_FUNCTION] {
        out.format(format_args!("type {} = (", function.alias))?;
        for (index, (name, ty)) in function.arguments.iter().enumerate() {
            if index > 0 {
                out.add(", ")?;
            }
            out.format(format_args!("{name}: "))?;
            host_value(out, ty)?;
        }
        out.add(if function.diverges {
            ") -> never\n"
        } else {
            ") -> ()\n"
        })?;
    }
    for slot in HookSlot::ALL {
        out.format(format_args!(
            "type {} = ChigiriReadContext & {{\n",
            api::alias(slot, "Context")
        ))?;
        fields(out, [(LOG_FUNCTION.name, LOG_FUNCTION.alias)])?;
        if slot.allows_reject() {
            fields(out, [(REJECT_FUNCTION.name, REJECT_FUNCTION.alias)])?;
        }
        fields(out, api::phase_fields(slot))?;
        out.add("}\n")?;
        out.format(format_args!("type {} = ", api::alias(slot, "Result")))?;
        if slot.allows_patch() {
            out.add("{\n")?;
            fields(out, [(api::PATCH, "ChigiriStatePatch")])?;
            out.add("} | ")?;
        }
        out.add("nil\n")?;
        out.format(format_args!("type {} = {{\n", api::alias(slot, "Hook")))?;
        out.key(api::RUN)?;
        out.format(format_args!(
            "(ctx: {}) -> {},\n}}\n",
            api::alias(slot, "Context"),
            api::alias(slot, "Result")
        ))?;
    }
    Ok(())
}

fn host_value(out: &mut Text, ty: &V) -> Result<(), ScriptError> {
    match ty {
        V::Text => out.add("string"),
        V::Number => out.add("number"),
        V::Json => out.add("ChigiriJson"),
        V::JsonObject => out.add("ChigiriJsonObject"),
        V::Null => out.add("ChigiriNull"),
        V::Named(name) => out.add(name),
        V::Array(item) => {
            out.add("{")?;
            host_value(out, item)?;
            out.add("}")
        }
        V::Optional(value) => {
            host_value(out, value)?;
            out.add("?")
        }
        V::Union(values) => {
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    out.add(" | ")?;
                }
                host_value(out, value)?;
            }
            Ok(())
        }
        V::Enumeration(values) => {
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    out.add(" | ")?;
                }
                out.quoted(value)?;
            }
            Ok(())
        }
    }
}

fn fields(
    out: &mut Text,
    fields: impl IntoIterator<Item = (&'static str, &'static str)>,
) -> Result<(), ScriptError> {
    for (name, ty) in fields {
        out.key(name)?;
        out.add(ty)?;
        out.add(",\n")?;
    }
    Ok(())
}

fn object(out: &mut Text, fields: &StateFields, patch: bool) -> Result<(), ScriptError> {
    out.add("{\n")?;
    for (name, ty) in fields {
        out.key(name)?;
        value(out, ty)?;
        if patch {
            out.add(" | nil")?;
        }
        out.add(",\n")?;
    }
    out.add("}")
}

fn value(out: &mut Text, ty: &StateType) -> Result<(), ScriptError> {
    match ty {
        StateType::String => out.add("string"),
        StateType::Boolean => out.add("boolean"),
        StateType::Integer | StateType::Number => out.add("number"),
        StateType::Object { fields } => object(out, fields, false),
        StateType::List { of } => {
            out.add("{")?;
            value(out, of)?;
            out.add("}")
        }
        StateType::Nullable { of } => {
            // Keep nullable values distinct from an omitted patch entry without redundant grouping.
            value(out, of)?;
            out.add(" | ChigiriNull")
        }
    }
}

/// Render one complete hook module with the slot-specific type and formatted source.
pub fn render_hook_module(slot: HookSlot, body: &str) -> Result<String, ScriptError> {
    let mut out = Text::new(MAX_BODY);
    out.format(format_args!(
        "--!strict\nlocal hook: {} = {{\n    {} = function(ctx: {}): {}\n",
        api::alias(slot, "Hook"),
        api::RUN,
        api::alias(slot, "Context"),
        api::alias(slot, "Result")
    ))?;
    out.add(body)?;
    out.add("\n    end,\n}\nreturn hook\n")?;
    super::format::source(&out.finish(), MAX_BODY)
}

/// Generate a no-op module for every hook slot without enabling registration.
pub fn static_hooks() -> Result<Vec<super::HookCandidate>, ScriptError> {
    HookSlot::ALL
        .into_iter()
        .map(|slot| {
            Ok(super::HookCandidate {
                slot,
                source: render_hook_module(
                    slot,
                    "-- Leave request state unchanged for this phase.\nreturn nil",
                )?,
            })
        })
        .collect()
}
