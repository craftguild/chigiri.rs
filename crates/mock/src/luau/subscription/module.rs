//! Render subscribe-only or subscribe-plus-resolve editable modules.

use crate::ir::{MockIr, ScriptId, ScriptRole};

use super::super::text::Text;
use super::super::{MAX_BODY, ScriptError, names};

/// Render a complete subscription resolver module with the role's exact entrypoints.
pub fn render_subscription_module(
    ir: &MockIr,
    id: ScriptId,
    subscribe_body: &str,
    resolve_body: Option<&str>,
) -> Result<String, ScriptError> {
    let plan = ir
        .scripts()
        .get(id.0)
        .ok_or_else(|| ScriptError::new("contract", "unknown script coordinate"))?;
    let ScriptRole::SubscriptionRoot { root_as_nested } = plan.role else {
        return Err(ScriptError::new(
            "contract",
            "subscription script coordinate required",
        ));
    };
    if root_as_nested != resolve_body.is_some() {
        return Err(ScriptError::new(
            "contract",
            "subscription entrypoint set does not match schema role",
        ));
    }
    let mut out = Text::new(MAX_BODY);
    out.format(format_args!(
        "--!strict\nlocal resolver: {} = {{\n    subscribe = function(",
        names::resolver(ir, id, "Resolver")
    ))?;
    super::declarations::subscribe_parameters(&mut out, ir, id)?;
    out.format(format_args!("): {}\n", names::resolver(ir, id, "Source")))?;
    out.add(subscribe_body)?;
    out.add("\n    end,\n")?;
    if let Some(resolve_body) = resolve_body {
        out.add("    resolve = function(")?;
        super::declarations::resolve_parameters(&mut out, ir, id)?;
        out.format(format_args!("): {}\n", names::resolver(ir, id, "Result")))?;
        out.add(resolve_body)?;
        out.add("\n    end,\n")?;
    }
    out.add("}\nreturn resolver\n")?;
    Ok(out.finish())
}
