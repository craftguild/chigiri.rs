//! Derive each coordinate's arguments, result, context type, and editable scaffold from one IR.

use super::reference::{self, Mode};
use super::text::Text;
use super::{MAX_BODY, ScriptError, names};
use crate::ir::{MockIr, ScriptId};

pub(super) fn declarations(out: &mut Text, ir: &MockIr, id: ScriptId) -> Result<(), ScriptError> {
    let plan = &ir.scripts()[id.0];
    let field = &ir.graph().fields()[plan.field.0];
    out.format(format_args!(
        "type {} = ",
        names::resolver(ir, id, "Result")
    ))?;
    reference::reference(out, ir, &field.ty, Mode::Output)?;
    out.format(format_args!(
        "\ntype {} = ",
        names::resolver(ir, id, "Context")
    ))?;
    out.add(
        if plan.role.is_mutation_root() && plan.role.root_as_nested() {
            "(ChigiriReadContext | ChigiriWriteContext)"
        } else if plan.role.is_mutation_root() {
            "ChigiriWriteContext"
        } else {
            "ChigiriReadContext"
        },
    )?;
    out.format(format_args!(
        "\ntype {} = {{\n",
        names::resolver(ir, id, "Resolver")
    ))?;
    out.add("    resolve: (")?;
    parameters(out, ir, id)?;
    out.format(format_args!(
        ") -> {},\n}}\n",
        names::resolver(ir, id, "Result")
    ))
}

fn parameters(out: &mut Text, ir: &MockIr, id: ScriptId) -> Result<(), ScriptError> {
    out.format(format_args!(
        "ctx: {}, args: {}",
        names::resolver(ir, id, "Context"),
        names::resolver(ir, id, "Args")
    ))?;
    let plan = &ir.scripts()[id.0];
    let field = &ir.graph().fields()[plan.field.0];
    if plan.role.root_as_nested()
        || (field.parent != ir.graph().query() && Some(field.parent) != ir.graph().mutation())
    {
        out.add(", parent: ")?;
        reference::name(out, ir, field.parent, Mode::Output)?;
        if plan.role.root_as_nested() {
            out.add("?")?;
        }
    }
    Ok(())
}

/// Render a complete resolver module for static candidates and executable fixtures.
/// Validation examines this standalone module rather than appending the body to a trusted prefix.
pub fn render_module(ir: &MockIr, id: ScriptId, body: &str) -> Result<String, ScriptError> {
    let Some(plan) = ir.scripts().get(id.0) else {
        return Err(ScriptError::new("contract", "unknown script coordinate"));
    };
    if plan.role.is_subscription_root() {
        return Err(ScriptError::new(
            "contract",
            "ordinary script coordinate required",
        ));
    }
    let mut out = Text::new(MAX_BODY);
    out.format(format_args!(
        "--!strict\nlocal resolver: {} = {{\n    resolve = function(",
        names::resolver(ir, id, "Resolver")
    ))?;
    parameters(&mut out, ir, id)?;
    out.format(format_args!("): {}\n", names::resolver(ir, id, "Result")))?;
    out.add(body)?;
    out.add("\n    end,\n}\nreturn resolver\n")?;
    Ok(out.finish())
}
