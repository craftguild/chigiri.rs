//! Parse bounded user SDL and append the built-in Mock directive vocabulary.

mod equivalence;
mod legacy;
mod limits;
mod snapshot;
#[cfg(test)]
mod tests;

pub use snapshot::parse_snapshot;
mod vocabulary;

use std::fmt;

use chigiri::core::language::{ast::Document, lex, parse_type_system_document};
use chigiri::core::{ExecutionLimits, Source, SourceId, Span};
use chigiri::experimental::OwnedSchema;

/// Maximum aggregate byte length of user SDL.
pub const MAX_SDL_BYTES: usize = 128 * 1024;

/// Checked-in SDL for Mock API vocabulary v3.
pub const VOCABULARY: &str = include_str!("vocabulary.graphql");
/// Current Mock API vocabulary version included in schema snapshots.
pub const VOCABULARY_VERSION: &str = "chigiri-mock-directives/v3";
/// Logical source name reserved for the synthetic vocabulary.
pub const VOCABULARY_SOURCE: &str = "chigiri-mock/v3.graphql";

/// Schema failure containing only a stable classification and source location.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaError {
    /// Stable failure classification.
    pub code: &'static str,
    /// Location in the original source, when available.
    pub span: Option<Span>,
}

impl SchemaError {
    pub(crate) const fn new(code: &'static str, span: Option<Span>) -> Self {
        Self { code, span }
    }
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mock schema: {}", self.code)
    }
}

impl std::error::Error for SchemaError {}

/// Validate user SDL and append the missing parts of the Mock vocabulary.
///
/// Enforce aggregate 128 KiB, 64-source, and schema-shape limits before composition.
/// Equivalent user-defined vocabulary declarations remain authoritative and are not duplicated.
/// A successful result is a valid GraphQL schema; the IR performs the remaining Mock-specific
/// semantic validation.
pub fn parse_sources(mut sources: Vec<Source>) -> Result<OwnedSchema, SchemaError> {
    limits::sources(&sources)?;
    let mut documents = Vec::with_capacity(sources.len());
    for (index, source) in sources.iter().enumerate() {
        documents.push(parse(index, source.text())?);
    }
    legacy::reject(&documents)?;
    let library = parse(sources.len(), VOCABULARY)?;
    let missing = vocabulary::missing(&documents, &library)?;
    limits::shapes(&documents, &library)?;
    sources.push(Source::new(VOCABULARY_SOURCE, missing));
    OwnedSchema::parse(sources).map_err(|error| {
        SchemaError::new(
            "invalid SDL",
            error
                .diagnostics()
                .and_then(|bag| bag.iter().next())
                .and_then(|d| d.span),
        )
    })
}

fn parse(index: usize, text: &str) -> Result<Document, SchemaError> {
    let source_id = SourceId::new(index);
    let output = parse_type_system_document(source_id, text, &ExecutionLimits::production());
    if let Some(document) = output.document {
        return Ok(document);
    }
    let tokens = lex(
        source_id,
        text,
        ExecutionLimits::production().max_diagnostics,
    );
    if tokens.is_valid() && tokens.tokens.len() == 1 {
        return Ok(Document {
            definitions: Vec::new(),
            span: Span::new(source_id, 0, 0),
        });
    }
    Err(SchemaError::new(
        "invalid SDL",
        output.diagnostics.iter().next().and_then(|d| d.span),
    ))
}
