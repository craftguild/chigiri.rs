//! Exact, bounded wire contract for `chigiri mock check --format json`.

use crate::diagnostic::Diagnostic;
use serde::{Deserialize, Serialize};

pub(super) const FORMAT: &str = "chigiri-mock-check/v3";
pub(super) const MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_REFERENCES: usize = 4;
const REFERENCE_TOPICS: [&str; 6] = [
    "consumers",
    "hooks",
    "models",
    "resolvers",
    "runtime",
    "types",
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CheckReport {
    pub format: String,
    pub scope: String,
    pub valid: bool,
    pub readiness_checked: bool,
    pub targets: Vec<String>,
    pub diagnostics: Vec<ReportDiagnostic>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReportDiagnostic {
    pub severity: String,
    pub file: Option<String>,
    pub coordinate: Option<String>,
    pub member: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub embedded: Option<ReportPosition>,
    pub phase: String,
    pub cause: String,
    pub expected: String,
    pub actual: String,
    pub hint: String,
    pub message: String,
    pub references: Vec<ReportReference>,
    #[serde(skip)]
    pub(super) navigation_hook: Option<String>,
    #[serde(skip)]
    pub(super) navigation_consumer: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReportPosition {
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReportReference {
    pub topic: String,
    pub name: String,
}

impl CheckReport {
    pub fn new(scope: &str, valid: bool, readiness_checked: bool) -> Self {
        Self {
            format: FORMAT.into(),
            scope: scope.into(),
            valid,
            readiness_checked,
            targets: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn validate(&self) -> Result<(), Diagnostic> {
        if self.format != FORMAT || !matches!(self.scope.as_str(), "project" | "targets") {
            return Err(contract_error());
        }
        if self.scope == "project" && !self.targets.is_empty()
            || self.scope == "targets" && self.readiness_checked
            || !self.valid && self.readiness_checked
        {
            return Err(contract_error());
        }
        for diagnostic in &self.diagnostics {
            diagnostic.validate()?;
        }
        Ok(())
    }
}

impl ReportDiagnostic {
    pub fn from_error(error: &Diagnostic) -> Self {
        Self {
            severity: "error".into(),
            file: error.file_known().then(|| error.file.to_string()),
            coordinate: error
                .coordinate_known()
                .then(|| error.coordinate.to_string()),
            member: error.member.clone(),
            line: error.position_known().then_some(error.toml.0),
            column: error.position_known().then_some(error.toml.1),
            embedded: error.embedded.as_deref().map(|position| ReportPosition {
                line: position.0,
                column: position.1,
            }),
            phase: error.phase.into(),
            cause: error.cause.into(),
            expected: error.expected.clone(),
            actual: error.actual.clone(),
            hint: error.hint.clone(),
            message: error.message.to_string(),
            references: Vec::new(),
            navigation_hook: error.hook_slot().map(|slot| slot.name().into()),
            navigation_consumer: error.consumer.clone(),
        }
    }

    pub(super) fn validate(&self) -> Result<(), Diagnostic> {
        if self.line.is_some() != self.column.is_some()
            || self.references.len() > MAX_REFERENCES
            || !self.references.windows(2).all(|pair| pair[0] < pair[1])
            || self.references.iter().any(|reference| {
                reference.name.is_empty() || !REFERENCE_TOPICS.contains(&reference.topic.as_str())
            })
        {
            return Err(contract_error());
        }
        Ok(())
    }
}

pub(super) fn encode(report: &CheckReport) -> Result<Vec<u8>, Diagnostic> {
    report.validate()?;
    let mut body = serde_json::to_vec(report)
        .map_err(|_| Diagnostic::environment("cannot encode check output"))?;
    body.push(b'\n');
    if body.len() > MAX_OUTPUT_BYTES {
        return Err(Diagnostic::environment("check output exceeds 16 MiB"));
    }
    Ok(body)
}

fn contract_error() -> Diagnostic {
    Diagnostic::environment("invalid check report contract")
}

#[cfg(test)]
#[path = "check_report_tests.rs"]
mod tests;
