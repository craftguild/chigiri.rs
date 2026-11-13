//! Owned-source budgets and integration with the existing SDL validator.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use chigiri_core::executor::ObjectScope;
use chigiri_core::language::{lex, parse_type_system_document};
use chigiri_core::schema::{TypeKind, compose_schema};
use chigiri_core::{DiagnosticBag, ExecutionLimits, SchemaDigest, Source, SourceId};
use sha2::{Digest as _, Sha256};

use super::{OwnedDefinition, OwnedSchema};

/// Diagnostic that omits source text and absolute paths from standard display.
pub struct OwnedSchemaError {
    message: &'static str,
    diagnostics: Option<DiagnosticBag>,
}

impl OwnedSchemaError {
    /// Parser or validator diagnostics with source locations.
    pub fn diagnostics(&self) -> Option<&DiagnosticBag> {
        self.diagnostics.as_ref()
    }

    fn boundary(message: &'static str) -> Self {
        Self {
            message,
            diagnostics: None,
        }
    }

    fn invalid(diagnostics: DiagnosticBag) -> Self {
        Self {
            message: "invalid owned schema",
            diagnostics: Some(diagnostics),
        }
    }
}

impl fmt::Display for OwnedSchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message)
    }
}

impl fmt::Debug for OwnedSchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl std::error::Error for OwnedSchemaError {}

impl OwnedSchema {
    /// Parse logical sources in order, with at most 65 sources and 256-byte names.
    ///
    /// The Mock layer validates at most 64 sources and 128 KiB of user SDL before adding
    /// its vocabulary source. Parsing uses production nesting and diagnostic budgets.
    /// Subscription execution additionally requires a matching source dispatch binding.
    pub fn parse(sources: Vec<Source>) -> Result<Self, OwnedSchemaError> {
        let limits = ExecutionLimits::production();
        if sources.is_empty() || sources.len() > 65 {
            return Err(OwnedSchemaError::boundary(
                "owned schema source count exceeded",
            ));
        }
        let mut names = BTreeSet::new();
        let mut bytes = 0usize;
        let mut digest = Sha256::new();
        // Bound all source bytes before parsing or copying them.
        for source in &sources {
            let name = source.name();
            if name.is_empty()
                || name.len() > 256
                || !names.insert(name)
                || name.starts_with('/')
                || name.contains('\\')
                || name
                    .split('/')
                    .any(|part| part.is_empty() || part == "." || part == "..")
                || name.chars().any(char::is_control)
            {
                return Err(OwnedSchemaError::boundary(
                    "invalid or duplicate logical source name",
                ));
            }
            bytes = bytes
                .checked_add(source.text().len())
                .filter(|bytes| *bytes <= limits.max_document_bytes)
                .ok_or_else(|| OwnedSchemaError::boundary("owned schema byte budget exceeded"))?;
            for value in [source.name().as_bytes(), source.text().as_bytes()] {
                digest.update((value.len() as u64).to_be_bytes());
                digest.update(value);
            }
        }
        let mut documents = Vec::with_capacity(sources.len());
        for (index, source) in sources.iter().enumerate() {
            let parsed = parse_type_system_document(SourceId::new(index), source.text(), &limits);
            if parsed.document.is_none() {
                let tokens = lex(SourceId::new(index), source.text(), limits.max_diagnostics);
                // Empty token streams are handled by schema composition with the other sources.
                if tokens.is_valid() && tokens.tokens.len() == 1 {
                    continue;
                }
            }
            documents.push(
                parsed
                    .document
                    .ok_or_else(|| OwnedSchemaError::invalid(parsed.diagnostics))?,
            );
        }
        let composed = compose_schema(&documents, &limits);
        let semantic = composed
            .schema
            .ok_or_else(|| OwnedSchemaError::invalid(composed.diagnostics))?;
        Ok(Self {
            inner: Arc::new(OwnedDefinition {
                object_names: semantic
                    .types()
                    .iter()
                    .filter(|ty| ty.kind() == TypeKind::Object)
                    .map(|ty| Arc::from(ty.name()))
                    .collect(),
                semantic: Arc::new(semantic),
                sources,
                digest: SchemaDigest::from_bytes(digest.finalize().into()),
                scope: ObjectScope::new(),
            }),
        })
    }
}
