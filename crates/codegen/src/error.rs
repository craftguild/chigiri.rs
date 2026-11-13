//! Owned structured diagnostics shared by the codegen library and CLI.

use std::fmt;

mod diagnostic;
mod policy;
pub use diagnostic::{CodegenDiagnostic, DiagnosticLabel, DiagnosticOrigin, DiagnosticSeverity};
pub use policy::DiagnosticPolicy;

/// Source-ordered collection of structured diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticReport {
    diagnostics: Vec<CodegenDiagnostic>,
    truncated: bool,
}

impl DiagnosticReport {
    /// Creates a report from diagnostics and an explicit truncation status.
    pub fn new(mut diagnostics: Vec<CodegenDiagnostic>, truncated: bool) -> Self {
        let truncated = truncated
            || diagnostics.len() > crate::diagnostic_render::MAX_RECORDS
            || diagnostics.iter().any(CodegenDiagnostic::truncated);
        diagnostics.truncate(crate::diagnostic_render::MAX_RECORDS);
        let mut report = Self {
            truncated,
            diagnostics,
        };
        // Propagate render byte-limit truncation into the structured model.
        report.truncated |= crate::diagnostic_render::exceeds_limits(&report);
        report
    }

    pub fn diagnostics(&self) -> &[CodegenDiagnostic] {
        &self.diagnostics
    }

    pub const fn truncated(&self) -> bool {
        self.truncated
    }
}

/// Code-generation error backed by a structured diagnostic report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodegenError {
    report: DiagnosticReport,
    legacy_message: String,
}

impl CodegenError {
    /// Creates an error from a stable code and message.
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        let message = message.into();
        Self::from_parts(
            DiagnosticReport::new(vec![CodegenDiagnostic::error(code, &message)], false),
            format!("{code}: {message}"),
        )
    }

    /// Creates an error from a structured report.
    pub fn from_report(mut report: DiagnosticReport) -> Self {
        if !report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity() == DiagnosticSeverity::Error)
        {
            report.diagnostics.push(CodegenDiagnostic::error(
                "SF4007",
                "CodegenError requires at least one error diagnostic.",
            ));
        }
        let legacy_message = render_legacy(&report);
        // Confidentiality and per-field limits are already applied to model input.
        Self {
            report,
            legacy_message,
        }
    }

    /// Convert the internal message that already contains the code to an error.
    pub(crate) fn from_message(message: impl Into<String>) -> Self {
        let message = message.into();
        let (code, detail) = split_legacy_message(&message);
        Self::from_parts(
            DiagnosticReport::new(vec![CodegenDiagnostic::error(code, detail)], false),
            message,
        )
    }

    fn from_parts(report: DiagnosticReport, legacy_message: String) -> Self {
        debug_assert!(
            report
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.severity() == DiagnosticSeverity::Error)
        );
        Self {
            report,
            legacy_message: crate::diagnostic_render::sanitize::text(&legacy_message),
        }
    }

    /// Returns the structured diagnostic report.
    pub const fn report(&self) -> &DiagnosticReport {
        &self.report
    }

    /// Consumes the error and returns its structured diagnostic report.
    pub fn into_report(self) -> DiagnosticReport {
        self.report
    }

    /// Returns the classified human-readable message.
    pub fn message(&self) -> &str {
        &self.legacy_message
    }
}

impl fmt::Display for CodegenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.legacy_message)
    }
}

impl std::error::Error for CodegenError {}

fn split_legacy_message(message: &str) -> (&str, &str) {
    if message.starts_with("error[SF") {
        let code = message.get(6..12).filter(|code| valid_code(code));
        if let Some(code) = code {
            let detail = message
                .get(14..)
                .unwrap_or_default()
                .lines()
                .next()
                .unwrap_or_default();
            return (code, detail.trim_start());
        }
    }
    let code = message.get(..6).filter(|code| valid_code(code));
    let Some(code) = code else {
        return ("SF4007", message);
    };
    let detail = message
        .get(6..)
        .unwrap_or_default()
        .strip_prefix(':')
        .unwrap_or_default()
        .trim_start();
    (code, detail)
}

fn valid_code(code: &str) -> bool {
    let bytes = code.as_bytes();
    bytes.len() == 6
        && bytes[0] == b'S'
        && bytes[1] == b'F'
        && matches!(bytes[2], b'1'..=b'6')
        && bytes[3..].iter().all(u8::is_ascii_digit)
}

fn render_legacy(report: &DiagnosticReport) -> String {
    crate::render_diagnostics(report, crate::DiagnosticFormat::Human)
}

#[cfg(test)]
mod tests {
    use super::{CodegenDiagnostic, CodegenError, DiagnosticReport};

    #[test]
    fn structured_reports_support_borrowing_and_owned_extraction() {
        let report = DiagnosticReport::new(
            vec![
                CodegenDiagnostic::error("SF4007", "Failed to produce output.")
                    .with_help("Please change the schema.")
                    .with_note("Diagnostics are in source order."),
            ],
            true,
        );
        let error = CodegenError::from_report(report);
        assert_eq!(error.report().diagnostics()[0].code(), "SF4007");
        assert_eq!(error.report().diagnostics()[0].help().len(), 1);
        assert!(error.report().truncated());
        assert_eq!(error.into_report().diagnostics().len(), 1);
    }

    #[test]
    fn legacy_messages_are_split_into_code_and_description() {
        let error = CodegenError::from_message("SF4002: There is no schema.");
        let diagnostic = &error.report().diagnostics()[0];
        assert_eq!(diagnostic.code(), "SF4002");
        assert_eq!(diagnostic.message(), "There is no schema.");
        assert_eq!(error.to_string(), "SF4002: There is no schema.");

        let rendered = CodegenError::from_message(
            "error[SF4011]: configuration is invalid\n --> chigiri.toml:1:1",
        );
        assert_eq!(rendered.report().diagnostics()[0].code(), "SF4011");
        assert_eq!(
            rendered.report().diagnostics()[0].message(),
            "configuration is invalid"
        );
    }

    #[test]
    fn appends_an_invariant_error_to_the_report() {
        let error = CodegenError::from_report(DiagnosticReport::new(
            vec![CodegenDiagnostic::note("SF4017", "note")],
            false,
        ));
        assert_eq!(error.report().diagnostics().len(), 2);
        assert_eq!(error.report().diagnostics()[1].code(), "SF4007");
    }

    #[test]
    fn structured_display_preserves_follow_up_causes_within_the_report_limit() {
        let diagnostics = (0..3)
            .map(|_| CodegenDiagnostic::error("SF4007", "a".repeat(8000)))
            .chain([CodegenDiagnostic::error("SF4024", "Rollback failed.")
                .with_help("The previous bytes were preserved.")])
            .collect();
        let error = CodegenError::from_report(DiagnosticReport::new(diagnostics, false));
        assert!(!error.report().truncated());
        let display = error.to_string();
        assert!(display.len() > 16 * 1024);
        assert!(display.len() < crate::MAX_DIAGNOSTIC_REPORT_BYTES);
        assert!(display.contains("Rollback failed."));
        assert!(display.contains("The previous bytes were preserved."));
    }
}
