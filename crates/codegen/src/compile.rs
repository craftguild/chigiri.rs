//! Parse discovered SDL and compose it into a semantic schema.

use chigiri_core::schema::{SemanticSchema, compose_schema};
use chigiri_core::{DiagnosticBag, ExecutionLimits, SourceMap};

use crate::discovery::DiscoveredSource;
use crate::error::{CodegenDiagnostic, CodegenError, DiagnosticLabel, DiagnosticReport};

pub(crate) struct CompiledSchema {
    pub(crate) sources: Vec<DiscoveredSource>,
    pub(crate) source_map: SourceMap,
    pub(crate) schema: SemanticSchema,
}

pub(crate) fn compile_sources(
    sources: Vec<DiscoveredSource>,
    limits: &ExecutionLimits,
) -> Result<CompiledSchema, CodegenError> {
    let mut source_map = SourceMap::new();
    for source in &sources {
        source_map.add(source.relative_path.as_str(), source.contents.as_str());
    }
    let mut documents = Vec::with_capacity(sources.len());
    for (source_id, source) in source_map.iter() {
        let output =
            chigiri_core::language::parse_type_system_document(source_id, source.text(), limits);
        let Some(document) = output.document else {
            return Err(diagnostics_error(&output.diagnostics, &source_map));
        };
        documents.push(document);
    }
    let output = compose_schema(&documents, limits);
    let Some(schema) = output.schema else {
        return Err(diagnostics_error(&output.diagnostics, &source_map));
    };
    Ok(CompiledSchema {
        sources,
        source_map,
        schema,
    })
}

fn diagnostics_error(diagnostics: &DiagnosticBag, sources: &SourceMap) -> CodegenError {
    if diagnostics.is_empty() {
        CodegenError::new(
            "SF4007",
            "The SDL processing stopped at the diagnostic limit.",
        )
    } else {
        let structured = diagnostics
            .iter()
            .map(|diagnostic| {
                let mut converted =
                    CodegenDiagnostic::error(diagnostic.code.as_str(), &diagnostic.message);
                if let Some(label) = diagnostic_label(diagnostic.span, sources) {
                    converted = converted.with_primary(label);
                }
                converted
            })
            .collect();
        CodegenError::from_report(DiagnosticReport::new(
            structured,
            diagnostics.was_truncated(),
        ))
    }
}

fn diagnostic_label(
    span: Option<chigiri_core::Span>,
    sources: &SourceMap,
) -> Option<DiagnosticLabel> {
    let span = span?;
    let source = sources.get(span.source_id)?;
    let location = source.location(span.start)?;
    let label = DiagnosticLabel::new(
        source.name(),
        span.start..span.end,
        location.line,
        location.column,
        "duplicate definition appears here",
    );
    Some(label)
}
