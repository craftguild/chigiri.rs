//! Redacts source text and rendered output from rustc diagnostics.

use serde_json::Value;

use crate::{CodegenDiagnostic, DiagnosticLabel, DiagnosticOrigin};

pub(super) fn convert(
    message: &Value,
    path: &impl Fn(&str) -> String,
) -> Option<CodegenDiagnostic> {
    let code = message
        .pointer("/code/code")
        .and_then(Value::as_str)
        .unwrap_or("SF4017");
    let text = message.get("message")?.as_str()?;
    let mut diagnostic = match message.get("level")?.as_str()? {
        "error" | "ice" => CodegenDiagnostic::error(code, text),
        "warning" => CodegenDiagnostic::warning(code, text),
        "note" | "help" | "failure-note" => CodegenDiagnostic::note(code, text),
        _ => return None,
    }
    .with_origin(DiagnosticOrigin::Rustc);
    for span in message.get("spans")?.as_array()? {
        let label = label(span, path)?;
        if span.get("is_primary")?.as_bool()? && diagnostic.primary().is_none() {
            diagnostic = diagnostic.with_primary(label);
        } else {
            diagnostic = diagnostic.with_secondary(label);
        }
    }
    let mut children = message
        .get("children")?
        .as_array()?
        .iter()
        .rev()
        .map(|child| (child, 0))
        .collect::<Vec<_>>();
    while let Some((child, depth)) = children.pop() {
        if depth > 64 {
            return None;
        }
        let text = child.get("message")?.as_str()?;
        diagnostic = if child.get("level")?.as_str()? == "help" {
            diagnostic.with_help(text)
        } else {
            diagnostic.with_note(text)
        };
        for span in child.get("spans")?.as_array()? {
            diagnostic = diagnostic.with_secondary(label(span, path)?);
        }
        children.extend(
            child
                .get("children")?
                .as_array()?
                .iter()
                .rev()
                .map(|child| (child, depth + 1)),
        );
    }
    Some(diagnostic)
}

fn label(span: &Value, path: &impl Fn(&str) -> String) -> Option<DiagnosticLabel> {
    Some(DiagnosticLabel::new(
        path(span.get("file_name")?.as_str()?),
        number(span, "byte_start")?..number(span, "byte_end")?,
        number(span, "line_start")?,
        number(span, "column_start")?,
        span.get("label")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    ))
}

fn number(value: &Value, field: &str) -> Option<usize> {
    usize::try_from(value.get(field)?.as_u64()?).ok()
}
