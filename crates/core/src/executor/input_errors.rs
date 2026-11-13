//! Requires at least one diagnostic before constructing a forced input failure.

use super::engine::Engine;
use crate::{DiagnosticBag, GraphQlError, PathSegment};

pub(super) fn rejection<C>(
    diagnostics: DiagnosticBag,
    engine: &Engine<'_, C>,
    path: &[PathSegment],
) -> Vec<GraphQlError> {
    let mut errors = Vec::new();
    for diagnostic in diagnostics {
        if !engine.claim_output_path(path) || !engine.claim_output_bytes(diagnostic.message.len()) {
            return vec![
                GraphQlError::new("Input error metadata resource limit").code("BAD_USER_INPUT"),
            ];
        }
        let mut error = GraphQlError::new(diagnostic.message).code("BAD_USER_INPUT");
        error.path = path.to_vec();
        if let Some(location) = diagnostic
            .span
            .and_then(|span| engine.source.location(span.start))
        {
            error.locations.push(location);
        }
        errors.push(error);
    }
    if errors.is_empty() {
        let mut error =
            GraphQlError::new("Input coercion failed after the diagnostic limit was reached")
                .code("BAD_USER_INPUT");
        if engine.claim_output_path(path) {
            error.path = path.to_vec();
        }
        errors.push(error);
    }
    errors
}
