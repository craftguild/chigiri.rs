//! Stable diagnostics shared across compiler phases.

mod render;

use crate::{SourceMap, Span};
use std::fmt;

/// Stable diagnostic code for clients and tooling.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DiagnosticCode(&'static str);

impl DiagnosticCode {
    /// Declare a stable code in the `SF1xxx` through `SF6xxx` ranges.
    ///
    /// An invalid literal is a programmer error rejected during const evaluation
    /// or initialization.
    pub const fn new(code: &'static str) -> Self {
        let bytes = code.as_bytes();
        assert!(
            bytes.len() == 6
                && bytes[0] == b'S'
                && bytes[1] == b'F'
                && bytes[2] >= b'1'
                && bytes[2] <= b'6'
                && is_ascii_digit(bytes[3])
                && is_ascii_digit(bytes[4])
                && is_ascii_digit(bytes[5]),
            "diagnostic code must match SF[1-6][0-9]{{3}}"
        );
        Self(code)
    }

    /// Return the code string.
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

const fn is_ascii_digit(byte: u8) -> bool {
    byte >= b'0' && byte <= b'9'
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

/// One compiler error with an optional primary source span.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    /// Stable code for machine inspection.
    pub code: DiagnosticCode,
    /// User-facing message.
    pub message: String,
    /// Primary source span, or `None` for errors independent of source text.
    pub span: Option<Span>,
}

impl Diagnostic {
    /// Create a diagnostic with a source span.
    pub fn new(code: DiagnosticCode, message: impl Into<String>, span: Span) -> Self {
        Self {
            code,
            message: message.into(),
            span: Some(span),
        }
    }

    /// Create a diagnostic independent of source text.
    pub fn without_span(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            span: None,
        }
    }

    /// Render a stable display with a source excerpt and caret.
    pub fn render(&self, sources: &SourceMap) -> String {
        render::render_diagnostic(self, sources)
    }
}

/// Diagnostic collection with a per-phase item limit.
#[derive(Clone, Debug)]
pub struct DiagnosticBag {
    diagnostics: Vec<Diagnostic>,
    max_diagnostics: usize,
    truncated: bool,
}

impl DiagnosticBag {
    /// Create an empty bag with the given maximum item count.
    pub const fn new(max_diagnostics: usize) -> Self {
        Self {
            diagnostics: Vec::new(),
            max_diagnostics,
            truncated: false,
        }
    }

    /// Add a diagnostic and return `true` while capacity remains; return `false`
    /// after the limit is reached.
    #[must_use]
    pub fn push(&mut self, diagnostic: Diagnostic) -> bool {
        if self.is_full() {
            self.truncated = true;
            return false;
        }
        self.diagnostics.push(diagnostic);
        true
    }

    /// Return the current number of diagnostics.
    pub fn len(&self) -> usize {
        self.diagnostics.len()
    }

    /// Whether the bag is empty.
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// Whether all available capacity has been used.
    pub fn is_full(&self) -> bool {
        self.diagnostics.len() >= self.max_diagnostics
    }

    /// Whether at least one diagnostic was discarded after reaching the limit.
    pub fn was_truncated(&self) -> bool {
        self.truncated
    }

    /// Whether the current phase should stop collecting diagnostics.
    ///
    /// With a zero limit, collection does not stop until an error is observed and
    /// `push` is attempted.
    pub(crate) fn should_stop(&self) -> bool {
        self.truncated || (!self.diagnostics.is_empty() && self.is_full())
    }

    /// Iterate over diagnostics in insertion order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &Diagnostic> {
        self.diagnostics.iter()
    }

    /// Render all diagnostics with source excerpts, separated by blank lines.
    pub fn render(&self, sources: &SourceMap) -> String {
        self.diagnostics
            .iter()
            .map(|diagnostic| diagnostic.render(sources))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Consume the bag and return its diagnostics.
    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.diagnostics
    }
}

impl IntoIterator for DiagnosticBag {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.diagnostics.into_iter()
    }
}

#[cfg(test)]
mod tests;
