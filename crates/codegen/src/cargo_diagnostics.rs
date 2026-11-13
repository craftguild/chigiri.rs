//! Convert Cargo JSON into a bounded model without retaining raw rendered text, source, or stderr.

#[cfg(test)]
mod redaction_tests;
mod rustc;
#[cfg(test)]
mod tests;

use serde_json::Value;

use crate::{
    CodegenDiagnostic, CodegenError, DiagnosticOrigin, DiagnosticReport, DiagnosticSeverity,
    ProcessOutput,
};

const MAX_RECORD_BYTES: usize = 1024 * 1024;
const MAX_MESSAGES: usize = 1000;

pub(crate) fn compile_output(
    output: &ProcessOutput,
    path: &impl Fn(&str) -> String,
) -> Result<DiagnosticReport, CodegenError> {
    compile_with(output, path, &|_| Vec::new())
}

pub(crate) fn compile_with(
    output: &ProcessOutput,
    path: &impl Fn(&str) -> String,
    supplements: &impl Fn(&Value) -> Vec<CodegenDiagnostic>,
) -> Result<DiagnosticReport, CodegenError> {
    let mut diagnostics = Vec::new();
    let mut seen_hints = Vec::new();
    let mut finished = None;
    let mut malformed = false;
    let mut truncated = false;
    for line in output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        if line.len() > MAX_RECORD_BYTES {
            truncated = true;
            break;
        }
        let Ok(record) = serde_json::from_slice::<Value>(line) else {
            malformed = true;
            break;
        };
        match record.get("reason").and_then(Value::as_str) {
            Some("compiler-message") if diagnostics.len() < MAX_MESSAGES => {
                match rustc::convert(&record["message"], path) {
                    Some(mut diagnostic) => {
                        let hints = supplements(&record);
                        let coordinates = hints
                            .iter()
                            .filter_map(CodegenDiagnostic::coordinate)
                            .collect::<std::collections::BTreeSet<_>>();
                        if coordinates.len() == 1 {
                            diagnostic = diagnostic.with_coordinate(*coordinates.first().unwrap());
                        }
                        diagnostics.push(diagnostic);
                        // With `--all-targets`, the library and test targets can report the same
                        // source diagnostic. Preserve only entries that differ in rustc details,
                        // location, or suggested replacement.
                        for hint in hints {
                            if !seen_hints.contains(&hint) {
                                seen_hints.push(hint.clone());
                                diagnostics.push(hint);
                            }
                        }
                    }
                    None => {
                        malformed = true;
                        break;
                    }
                }
            }
            Some("compiler-message") => {
                truncated = true;
                break;
            }
            Some("build-finished") if finished.is_none() => {
                finished = record.get("success").and_then(Value::as_bool);
                if finished.is_none() {
                    malformed = true;
                    break;
                }
            }
            Some("compiler-artifact" | "build-script-executed") => {}
            _ => {
                malformed = true;
                break;
            }
        }
    }
    if malformed || finished != Some(output.status.success()) {
        diagnostics.push(
            CodegenDiagnostic::error(
                "SF4017",
                "Cargo did not report a consistent completion status. Check the Cargo environment and lockfile.",
            )
            .with_origin(DiagnosticOrigin::Cargo)
            .with_help("Run the required Cargo command again after making its dependencies and lockfile available."),
        );
    }
    let compiler_errors = diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity() == DiagnosticSeverity::Error);
    if !output.status.success() && !compiler_errors {
        diagnostics.push(
            CodegenDiagnostic::error(
                "SF4017",
                "I couldn't run the cargo properly. Please check the normal cargo environment.",
            )
            .with_origin(DiagnosticOrigin::Cargo),
        );
    }
    let mut report = DiagnosticReport::new(diagnostics, truncated);
    if report.truncated() {
        let mut diagnostics = vec![
            CodegenDiagnostic::error("SF4017", "Cargo diagnostics exceeded the configured limit.")
                .with_origin(DiagnosticOrigin::Cargo),
        ];
        diagnostics.extend(report.diagnostics().iter().take(MAX_MESSAGES - 1).cloned());
        report = DiagnosticReport::new(diagnostics, true);
    }
    if report.truncated()
        || report
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.severity() == DiagnosticSeverity::Error)
    {
        Err(CodegenError::from_report(report))
    } else {
        Ok(report)
    }
}
