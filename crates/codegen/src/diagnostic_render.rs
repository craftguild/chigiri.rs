//! Redact diagnostics consistently and render them as one bounded report.

pub(crate) mod sanitize;
#[cfg(test)]
mod tests;

use serde_json::{Value, json};

use crate::{DiagnosticOrigin, DiagnosticReport, DiagnosticSeverity};

/// CLI is sending a diagnostic wire format to stderr.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DiagnosticFormat {
    #[default]
    Human,
    Json,
}

/// Report schema shared by all commands. The format is one JSON document, not JSON Lines.
pub const DIAGNOSTIC_REPORT_SCHEMA: &str = "chigiri/diagnostics-v2";
/// Maximum UTF-8 bytes in a rendered report.
pub const MAX_DIAGNOSTIC_REPORT_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_RECORDS: usize = 1000;
pub(crate) const MAX_LABELS: usize = 32;
pub(crate) const MAX_SUPPLEMENTS: usize = 32;

pub(crate) fn exceeds_limits(report: &DiagnosticReport) -> bool {
    bounded_records(report).1
}

/// The raw source excerpt does not hold or draw.
pub fn render_diagnostics(report: &DiagnosticReport, format: DiagnosticFormat) -> String {
    let (records, truncated) = bounded_records(report);
    match format {
        DiagnosticFormat::Json => json!({
            "schema_version": DIAGNOSTIC_REPORT_SCHEMA,
            "diagnostics": records,
            "truncated": truncated,
        })
        .to_string(),
        DiagnosticFormat::Human => render_human(&records),
    }
}

fn bounded_records(report: &DiagnosticReport) -> (Vec<Value>, bool) {
    let mut records = Vec::new();
    let mut bytes = 0usize;
    let mut truncated = report.truncated();
    for diagnostic in report.diagnostics() {
        let labels = diagnostic
            .primary()
            .into_iter()
            .map(|label| (label, true))
            .chain(diagnostic.secondary().iter().map(|label| (label, false)));
        let label_count =
            usize::from(diagnostic.primary().is_some()) + diagnostic.secondary().len();
        truncated |= label_count > MAX_LABELS
            || diagnostic.help().len() > MAX_SUPPLEMENTS
            || diagnostic.notes().len() > MAX_SUPPLEMENTS;
        let record = json!({
            "origin": match diagnostic.origin() {
                DiagnosticOrigin::Chigiri => "chigiri",
                DiagnosticOrigin::Rustc => "rustc",
                DiagnosticOrigin::Cargo => "cargo",
            },
            "code": diagnostic.code(),
            "severity": severity_name(diagnostic.severity()),
            "message": diagnostic.message(),
            "coordinate": diagnostic.coordinate(),
            "labels": labels.take(MAX_LABELS).map(|(label, primary)| json!({
                "primary": primary, "path": label.path(), "start": label.start(), "end": label.end(),
                "line": label.line(), "column": label.column(), "message": label.message(),
            })).collect::<Vec<_>>(),
            "help": diagnostic.help().iter().take(MAX_SUPPLEMENTS).collect::<Vec<_>>(),
            "notes": diagnostic.notes().iter().take(MAX_SUPPLEMENTS).collect::<Vec<_>>(),
        });
        let size = record.to_string().len() + 1;
        if records.len() >= MAX_RECORDS || bytes + size > MAX_DIAGNOSTIC_REPORT_BYTES - 1024 {
            truncated = true;
            break;
        }
        bytes += size;
        records.push(record);
    }
    if truncated {
        if records.len() == MAX_RECORDS {
            records.pop();
        }
        records.push(json!({
            "origin": "chigiri", "code": "SF4007", "severity": "error",
            "message": "The diagnostic limit was reached; additional diagnostics were omitted.",
            "coordinate": null, "labels": [], "help": [], "notes": [],
        }));
    }
    (records, truncated)
}

fn render_human(records: &[Value]) -> String {
    let mut output = String::new();
    for record in records {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(&format!(
            "{}[{}]: {}\n",
            string(record, "severity"),
            string(record, "code"),
            string(record, "message")
        ));
        if let Some(coordinate) = record["coordinate"].as_str() {
            output.push_str(&format!("coordinate: {coordinate}\n"));
        }
        for label in record["labels"].as_array().unwrap() {
            let prefix = if label["primary"] == true {
                " -->"
            } else {
                " :::"
            };
            output.push_str(&format!(
                "{prefix} {}:{}:{} {}\n",
                string(label, "path"),
                label["line"],
                label["column"],
                string(label, "message")
            ));
        }
        for (name, prefix) in [("help", "help"), ("notes", "note")] {
            for value in record[name].as_array().unwrap() {
                output.push_str(&format!("{prefix}: {}\n", value.as_str().unwrap()));
            }
        }
    }
    output.trim_end().to_owned()
}

fn string<'a>(record: &'a Value, name: &str) -> &'a str {
    record[name].as_str().unwrap()
}

const fn severity_name(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Error => "error",
        DiagnosticSeverity::Warning => "warning",
        DiagnosticSeverity::Note => "note",
    }
}
