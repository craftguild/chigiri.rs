//! Test bounded parsing and redaction with byte input representative of the real tool.

use serde_json::{Value, json};

use crate::{
    DiagnosticFormat, DiagnosticPolicy, DiagnosticSeverity, ProcessOutput, render_diagnostics,
};

pub(super) fn message(level: &str) -> Value {
    json!({"reason":"compiler-message","message":{
        "code":{"code":"E0308"}, "level":level, "message":"mismatched types",
        "rendered":"raw-private-source", "spans":[{"file_name":"src/lib.rs","byte_start":1,"byte_end":2,"line_start":2,"column_start":3,"is_primary":true,"label":"expected type","text":[{"text":"raw-private-source"}]}],
        "children":[{"level":"help","message":"The first amendment.","spans":[],"children":[]},{"level":"note","message":"Next addition.","spans":[],"children":[]}]
    }})
}

fn serialized(records: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for record in records {
        serde_json::to_writer(&mut bytes, record).unwrap();
        bytes.push(b'\n');
    }
    bytes
}

#[cfg(unix)]
pub(super) fn output(records: &[Value], success: bool) -> ProcessOutput {
    use std::os::unix::process::ExitStatusExt as _;
    ProcessOutput {
        status: std::process::ExitStatus::from_raw(if success { 0 } else { 256 }),
        stdout: serialized(records),
        stderr: b"password=raw-private-stderr".to_vec(),
    }
}

#[cfg(windows)]
pub(super) fn output(records: &[Value], success: bool) -> ProcessOutput {
    use std::os::windows::process::ExitStatusExt as _;
    ProcessOutput {
        status: std::process::ExitStatus::from_raw(if success { 0 } else { 1 }),
        stdout: serialized(records),
        stderr: b"password=raw-private-stderr".to_vec(),
    }
}

#[test]
fn sg_cli_07_preserves_rustc_code_severity_labels_help_notes_and_raw_source() {
    let output = output(
        &[
            message("error"),
            json!({"reason":"build-finished","success":false}),
        ],
        false,
    );
    let error = super::compile_output(&output, &str::to_owned).unwrap_err();
    let diagnostic = &error.report().diagnostics()[0];
    assert_eq!(diagnostic.code(), "E0308");
    assert_eq!(diagnostic.severity(), DiagnosticSeverity::Error);
    assert_eq!(diagnostic.primary().unwrap().path(), "src/lib.rs");
    assert_eq!(diagnostic.primary().unwrap().start(), 1);
    assert_eq!(diagnostic.help(), ["The first amendment."]);
    assert_eq!(diagnostic.notes(), ["Next addition."]);
    for format in [DiagnosticFormat::Json, DiagnosticFormat::Human] {
        assert!(!render_diagnostics(error.report(), format).contains("raw-private"));
    }
}

#[test]
fn sg_cli_07_redacts_rustc_text_and_unmodeled_cargo_secrets() {
    let secret = "path=/outside/private.rs https://user:private@example.org token=private-token";
    let mut record = message("error");
    for pointer in [
        "/message/message",
        "/message/spans/0/file_name",
        "/message/spans/0/label",
        "/message/children/0/message",
        "/message/children/1/message",
    ] {
        *record.pointer_mut(pointer).unwrap() = secret.into();
    }
    let mut samples = vec![output(
        &[record, json!({"reason":"build-finished","success":false})],
        false,
    )];
    let mut malformed = output(&[], false);
    malformed.stdout = format!("{{ invalid {secret}\n").into_bytes();
    samples.push(malformed);
    for output in samples {
        let error = super::compile_output(&output, &str::to_owned).unwrap_err();
        for format in [DiagnosticFormat::Json, DiagnosticFormat::Human] {
            let rendered = render_diagnostics(error.report(), format);
            for forbidden in ["private", "example.org", "/outside", "://"] {
                assert!(!rendered.contains(forbidden), "{rendered}");
            }
        }
        assert!(!format!("{error:?}").contains("private"));
    }
}

#[test]
fn sg_proc_01_rejects_missing_duplicate_failed_and_malformed_completion_records() {
    for records in [
        vec![],
        vec![json!({"reason":"build-finished","success":false})],
        vec![json!({"reason":"build-finished","success":true}); 2],
        vec![json!({"reason":"compiler-message","message":{}})],
        vec![json!({"reason":"unknown"})],
    ] {
        let error = super::compile_output(&output(&records, true), &str::to_owned).unwrap_err();
        assert!(
            error
                .report()
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code() == "SF4017")
        );
    }
}

#[test]
fn sg_proc_01_fails_closed_when_cargo_output_or_diagnostic_count_exceeds_limits() {
    let mut oversized = output(&[], true);
    oversized.stdout = vec![b'x'; 1024 * 1024 + 1];
    let many = output(&vec![message("warning"); 1001], true);
    for output in [oversized, many] {
        let error = super::compile_output(&output, &str::to_owned).unwrap_err();
        assert!(error.report().truncated());
        assert!(
            render_diagnostics(error.report(), DiagnosticFormat::Json).len()
                <= crate::MAX_DIAGNOSTIC_REPORT_BYTES
        );
    }
}

#[test]
fn diagnostic_policy_can_deny_warnings() {
    for (level, allowed) in [("warning", false), ("note", true)] {
        let report = super::compile_output(
            &output(
                &[
                    message(level),
                    json!({"reason":"build-finished","success":true}),
                ],
                true,
            ),
            &str::to_owned,
        )
        .unwrap();
        assert!(DiagnosticPolicy::default().enforce(report.clone()).is_ok());
        assert_eq!(
            DiagnosticPolicy {
                deny_warnings: true
            }
            .enforce(report)
            .is_ok(),
            allowed
        );
    }
}

#[test]
fn keeps_distinct_lib_and_test_diagnostics_and_deduplicates_only_exact_matches() {
    let first = message("error");
    let mut different = first.clone();
    different["message"]["spans"][0]["byte_start"] = 7.into();
    different["message"]["spans"][0]["byte_end"] = 8.into();
    let output = output(
        &[
            first.clone(),
            first,
            different,
            json!({"reason":"build-finished","success":false}),
        ],
        false,
    );
    let error = super::compile_with(&output, &str::to_owned, &|record| {
        let start = record["message"]["spans"][0]["byte_start"]
            .as_u64()
            .unwrap() as usize;
        vec![
            crate::CodegenDiagnostic::error("SF4019", "Please change the type.")
                .with_coordinate("User.id")
                .with_primary(crate::DiagnosticLabel::new(
                    "src/lib.rs",
                    start..start + 1,
                    2,
                    3,
                    "consumer source",
                )),
        ]
    })
    .unwrap_err();
    let diagnostics = error.report().diagnostics();
    assert_eq!(
        diagnostics
            .iter()
            .filter(|record| record.code() == "E0308")
            .count(),
        3
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|record| record.code() == "SF4019")
            .count(),
        2
    );
    assert!(
        diagnostics
            .iter()
            .all(|record| record.coordinate() == Some("User.id"))
    );
    assert!(!error.report().truncated());
}
