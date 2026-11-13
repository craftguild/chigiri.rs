//! Structured diagnostics with severity, origin, source labels, and supplemental guidance.

use crate::diagnostic_render::{MAX_LABELS, MAX_SUPPLEMENTS, sanitize};
use crate::filesystem::PortableRelativePath;

/// Diagnostic severity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Note,
}

/// Diagnostic origin, represented independently from untrusted tool output.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DiagnosticOrigin {
    #[default]
    Chigiri,
    Rustc,
    Cargo,
}

/// One label on the source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticLabel {
    truncated: bool,
    redacted_path: bool,
    path: PortableRelativePath,
    start: usize,
    end: usize,
    line: usize,
    column: usize,
    message: String,
}

impl DiagnosticLabel {
    /// Creates a label from a normalized relative path and byte/display position.
    pub fn new(
        path: impl Into<String>,
        span: std::ops::Range<usize>,
        line: usize,
        column: usize,
        message: impl Into<String>,
    ) -> Self {
        let path = path.into();
        let message = message.into();
        let path_truncated = sanitize::exceeds_limit(&path);
        let sanitized = sanitize::path(&path);
        let (path, redacted_path) = match PortableRelativePath::new(&sanitized) {
            Ok(path) => (path, false),
            Err(_) => (
                PortableRelativePath::new("redacted-path")
                    .expect("fixed diagnostic path is portable"),
                true,
            ),
        };
        Self {
            truncated: path_truncated || sanitize::exceeds_limit(&message),
            redacted_path,
            path,
            start: span.start,
            end: span.end,
            line,
            column,
            message: sanitize::text(&message),
        }
    }

    pub fn path(&self) -> &str {
        if self.redacted_path {
            "<path>"
        } else {
            self.path.as_str()
        }
    }

    pub const fn start(&self) -> usize {
        self.start
    }

    pub const fn end(&self) -> usize {
        self.end
    }

    pub const fn line(&self) -> usize {
        self.line
    }

    pub const fn column(&self) -> usize {
        self.column
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Structured Chigiri diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodegenDiagnostic {
    truncated: bool,
    origin: DiagnosticOrigin,
    coordinate: Option<String>,
    code: String,
    severity: DiagnosticSeverity,
    message: String,
    primary: Option<DiagnosticLabel>,
    secondary: Vec<DiagnosticLabel>,
    help: Vec<String>,
    notes: Vec<String>,
}

impl CodegenDiagnostic {
    /// Creates an error diagnostic without a source span.
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(DiagnosticSeverity::Error, code, message)
    }

    /// Creates a tool note that can appear in the diagnostic stream.
    pub fn note(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(DiagnosticSeverity::Note, code, message)
    }

    /// Creates a warning that fails when deny-warnings is enabled.
    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(DiagnosticSeverity::Warning, code, message)
    }

    fn new(
        severity: DiagnosticSeverity,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        let code = code.into();
        let message = message.into();
        Self {
            truncated: sanitize::exceeds_limit(&code) || sanitize::exceeds_limit(&message),
            origin: DiagnosticOrigin::Chigiri,
            coordinate: None,
            code: sanitize::text(&code),
            severity,
            message: sanitize::text(&message),
            primary: None,
            secondary: Vec::new(),
            help: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// Sets the primary label.
    pub fn with_primary(mut self, label: DiagnosticLabel) -> Self {
        self.truncated |= label.truncated;
        self.primary = Some(label);
        self
    }

    /// Appends a secondary label in source order.
    pub fn with_secondary(mut self, label: DiagnosticLabel) -> Self {
        self.truncated |= label.truncated;
        if self.secondary.len() == MAX_LABELS {
            self.truncated = true;
        } else {
            self.secondary.push(label);
        }
        self
    }

    /// Appends a remediation separate from the main message.
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        let help = help.into();
        self.truncated |= sanitize::exceeds_limit(&help);
        if self.help.len() == MAX_SUPPLEMENTS {
            self.truncated = true;
        } else {
            self.help.push(sanitize::text(&help));
        }
        self
    }

    /// Appends supplementary information separate from the main message.
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        let note = note.into();
        self.truncated |= sanitize::exceeds_limit(&note);
        if self.notes.len() == MAX_SUPPLEMENTS {
            self.truncated = true;
        } else {
            self.notes.push(sanitize::text(&note));
        }
        self
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    /// Mark a rustc- or Cargo-derived diagnostic stored in the shared report.
    pub fn with_origin(mut self, origin: DiagnosticOrigin) -> Self {
        self.origin = origin;
        self
    }

    /// Attach an SDL coordinate when one is available.
    pub fn with_coordinate(mut self, coordinate: impl Into<String>) -> Self {
        let coordinate = coordinate.into();
        self.truncated |= sanitize::exceeds_limit(&coordinate);
        self.coordinate = Some(sanitize::text(&coordinate));
        self
    }

    pub(crate) const fn truncated(&self) -> bool {
        self.truncated
    }

    pub const fn origin(&self) -> DiagnosticOrigin {
        self.origin
    }

    pub fn coordinate(&self) -> Option<&str> {
        self.coordinate.as_deref()
    }

    pub const fn severity(&self) -> DiagnosticSeverity {
        self.severity
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub const fn primary(&self) -> Option<&DiagnosticLabel> {
        self.primary.as_ref()
    }

    pub fn secondary(&self) -> &[DiagnosticLabel] {
        &self.secondary
    }

    pub fn help(&self) -> &[String] {
        &self.help
    }

    pub fn notes(&self) -> &[String] {
        &self.notes
    }
}
