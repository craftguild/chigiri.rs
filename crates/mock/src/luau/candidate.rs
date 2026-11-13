//! Produce explicit manual resolver scaffolds without inferring user behavior.

use super::{MAX_BODY, ScriptError};
use crate::ir::{MockIr, ScriptId};

/// User-owned candidate input consumed by the same validation path as generated scaffolds.
pub struct ScriptCandidate {
    /// IR position of the canonical coordinate.
    pub script: ScriptId,
    /// Complete editable Luau module returning the closed `resolve` table.
    pub source: String,
}

/// Convert each resolved script plan into a static candidate without guessing behavior from names.
pub fn static_candidates(ir: &MockIr) -> Result<Vec<ScriptCandidate>, ScriptError> {
    ir.scripts().iter().enumerate().map(|(id, _)| {
        let script = ScriptId(id);
        let field = &ir.graph().fields()[ir.scripts()[id].field.0];
        let arguments = field
            .arguments
            .iter()
            .map(|argument| ir.graph().inputs()[argument.0].name.as_ref())
            .collect::<Vec<_>>()
            .join(",");
        let failure = "ctx.fail({\n    code = \"NOT_IMPLEMENTED\",\n    message = \"This operation is not implemented.\",\n})";
        let plan = &ir.scripts()[id];
        let call = if plan.role.root_as_nested() && plan.role.is_mutation_root() {
            format!(
                "if ctx.scope == \"mutation\" then\n    {failure}\nelse\n    {failure}\nend"
            )
        } else {
            failure.into()
        };
        let body = format!("-- Signature: {}({arguments})\n{call}\n", field.name);
        let source = if plan.role.is_subscription_root() {
            let body = format!("-- Signature: {}({arguments})\nreturn {failure}\n", field.name);
            super::render_subscription_module(
                ir,
                script,
                &body,
                plan.role.root_as_nested().then_some(body.as_str()),
            )?
        } else {
            super::render_module(ir, script, &body)?
        };
        let source = super::format::source(&source, MAX_BODY).map_err(|mut error| {
            error.script = Some(script);
            error
        })?;
        Ok(ScriptCandidate { script, source })
    }).collect()
}
