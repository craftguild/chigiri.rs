//! Reserves long input paths before allocating diagnostics, then falls back to a short fixed diagnostic.

use super::{INPUT_RESOURCE_LIMIT, InputCoercionBudget};
use crate::{Diagnostic, DiagnosticCode, Span};

pub(super) fn build(
    code: DiagnosticCode,
    message: &str,
    span: Option<Span>,
    path: &[String],
    budget: Option<&InputCoercionBudget>,
) -> (Diagnostic, bool) {
    let path_bytes = path.iter().fold(0usize, |bytes, segment| {
        bytes
            .saturating_add(usize::from(bytes != 0 && !segment.starts_with('[')))
            .saturating_add(segment.len())
    });
    let bytes = message
        .len()
        .saturating_add(path_bytes)
        .saturating_add(if path.is_empty() { 0 } else { 4 });
    let exhausted = budget.is_some_and(|budget| !budget.claim(0, bytes));
    let (code, text) = if exhausted {
        (
            INPUT_RESOURCE_LIMIT,
            "Input coercion resource limit exceeded".to_owned(),
        )
    } else {
        let mut text = String::with_capacity(bytes);
        text.push_str(message);
        if !path.is_empty() {
            text.push_str(" at ");
            let start = text.len();
            for segment in path {
                if text.len() != start && !segment.starts_with('[') {
                    text.push('.');
                }
                text.push_str(segment);
            }
        }
        (code, text)
    };
    let diagnostic = match span {
        Some(span) => Diagnostic::new(code, text, span),
        None => Diagnostic::without_span(code, text),
    };
    (diagnostic, exhausted)
}
