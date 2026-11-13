//! Applies display limits and warning policy to pre-commit diagnostics.

use super::{CodegenDiagnostic, CodegenError, DiagnosticReport, DiagnosticSeverity};

/// Shared diagnostic policy that leaves production settings unchanged.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DiagnosticPolicy {
    /// Treats warnings as errors and prevents transaction commit.
    pub deny_warnings: bool,
}

impl DiagnosticPolicy {
    pub(crate) fn enforce(
        self,
        report: DiagnosticReport,
    ) -> Result<DiagnosticReport, CodegenError> {
        if report.truncated()
            || report
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.severity() == DiagnosticSeverity::Error)
        {
            return Err(CodegenError::from_report(report));
        }
        if self.deny_warnings
            && report
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.severity() == DiagnosticSeverity::Warning)
        {
            let mut diagnostics = report.diagnostics().to_vec();
            diagnostics.push(CodegenDiagnostic::error(
                "SF4028",
                "deny-warnings is a failure.",
            ));
            return Err(CodegenError::from_report(DiagnosticReport::new(
                diagnostics,
                false,
            )));
        }
        Ok(report)
    }
}
