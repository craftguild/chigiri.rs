//! Consolidated path-redaction cases for the diagnostic report adapter.

use serde_json::json;

use super::tests::{message, output};
use crate::{DiagnosticFormat, render_diagnostics};

const PATHS: &str = concat!(
    "path=/outside/secret.rs file:///outside/secret.rs ",
    "file:///C:/Users/alice/secret.rs. 'file://server/share/secret.rs', ",
    "file://[::1]/share/secret.rs; file:///C:/Users/alice/a,b/secret.rs, ",
    "see:file:///C:/Users/alice/secret.rs! path:/absolute/path! ",
    "/tmp/user,name/private.rs /tmp/user:group/private.rs ",
    "source:/outside/source-private.rs cwd:C:\\Users\\alice\\private.rs ",
    "https://example.com/docs/reference Z:\\outside\\secret.rs \\\\server\\share\\secret.rs\n",
    "quoted=\"/outside/secret.rs\", \x1b[31mvalue\x1b[0m"
);

fn verify(value: &str) {
    for forbidden in [
        "/outside",
        "secret.rs",
        "private.rs",
        "C:/Users",
        "server/share",
        "[::1]",
        "a,b",
        "user,name",
        "user:group",
        "/absolute/path",
        "example.com",
        "Z:\\",
        "\\\\server",
        "\x1b",
    ] {
        assert!(!value.contains(forbidden), "The secret failure:");
    }
}

#[test]
fn diag_03_redacts_all_rustc_text_fields_and_raw_source_in_human_and_json_output() {
    let mut record = message("error");
    for pointer in [
        "/message/message",
        "/message/spans/0/label",
        "/message/children/0/message",
        "/message/children/1/message",
    ] {
        *record.pointer_mut(pointer).unwrap() = PATHS.into();
    }
    record["message"]["spans"][0]["file_name"] = "file:///C:/Users/alice/secret.rs".into();
    let error = super::compile_output(
        &output(
            &[record, json!({"reason":"build-finished","success":false})],
            false,
        ),
        &crate::diagnostic_render::sanitize::path,
    )
    .unwrap_err();
    for format in [DiagnosticFormat::Human, DiagnosticFormat::Json] {
        let rendered = render_diagnostics(error.report(), format);
        verify(&rendered);
        assert!(!rendered.contains("raw-private-source"));
        assert!(rendered.contains("E0308"));
    }
}

#[test]
fn diag_03_malformed_cargo_and_ansi_are_redacted_from_raw_notes() {
    let mut output = output(&[], false);
    output.stdout = format!("bad JSON {PATHS}\nline").into_bytes();
    let error = super::compile_output(&output, &str::to_owned).unwrap_err();
    for format in [DiagnosticFormat::Human, DiagnosticFormat::Json] {
        let rendered = render_diagnostics(error.report(), format);
        verify(&rendered);
        assert!(!rendered.contains("bad JSON"));
        assert!(rendered.contains("SF4017"));
    }
}

#[test]
fn cargo_stream_parse_failures_use_the_same_diagnostic_redaction_boundary() {
    let mut output = output(
        &[
            message("note"),
            json!({"reason":"build-finished","success":true}),
        ],
        true,
    );
    output.stdout.splice(0..0, b"\n\n".iter().copied());
    output.stdout.extend_from_slice(b"\n\n");
    let report = super::compile_output(&output, &str::to_owned).unwrap();
    assert_eq!(report.diagnostics().len(), 1);
}
