//! Verifies wire shape, confidentiality, and diagnostic limits from the same
//! public model.

use super::{DIAGNOSTIC_REPORT_SCHEMA, MAX_DIAGNOSTIC_REPORT_BYTES, render_diagnostics};
use crate::{
    CodegenDiagnostic, CodegenError, DiagnosticFormat, DiagnosticLabel, DiagnosticOrigin,
    DiagnosticPolicy, DiagnosticReport,
};
use serde_json::Value;

fn json(report: &DiagnosticReport) -> Value {
    let output = render_diagnostics(report, DiagnosticFormat::Json);
    assert!(output.len() <= MAX_DIAGNOSTIC_REPORT_BYTES);
    assert_eq!(output.lines().count(), 1);
    serde_json::from_str(&output).unwrap()
}

#[test]
fn structured_report_preserves_origins_coordinates_and_complete_labels() {
    let error = CodegenDiagnostic::error("E0046", "A required method is missing.")
        .with_origin(DiagnosticOrigin::Rustc)
        .with_coordinate("Query.book")
        .with_primary(DiagnosticLabel::new(
            "src/lib.rs",
            8..12,
            2,
            3,
            "Implementation site.",
        ))
        .with_secondary(DiagnosticLabel::new(
            "graphql/book.graphql",
            0..4,
            1,
            1,
            "SDL definition",
        ))
        .with_help("Use Types.")
        .with_note("Compile one generated crate first.");
    let report = DiagnosticReport::new(
        vec![
            error,
            CodegenDiagnostic::note("SF4017", "Cargo check completed.")
                .with_origin(DiagnosticOrigin::Cargo),
        ],
        false,
    );
    let value = json(&report);
    assert_eq!(value.as_object().unwrap().len(), 3);
    assert_eq!(value["schema_version"], DIAGNOSTIC_REPORT_SCHEMA);
    assert_eq!(value["truncated"], false);
    assert_eq!(value["diagnostics"].as_array().unwrap().len(), 2);
    let error = &value["diagnostics"][0];
    assert_eq!(error.as_object().unwrap().len(), 8);
    assert_eq!(error["origin"], "rustc");
    assert_eq!(error["coordinate"], "Query.book");
    assert_eq!(error["labels"][0].as_object().unwrap().len(), 7);
    assert_eq!(error["labels"][0]["primary"], true);
    assert_eq!(error["labels"][1]["primary"], false);
    assert_eq!(value["diagnostics"][1]["origin"], "cargo");
    let human = render_diagnostics(&report, DiagnosticFormat::Human);
    for fact in [
        "E0046",
        "Query.book",
        "src/lib.rs:2:3",
        "graphql/book.graphql:1:1",
        "help: Use Types.",
        "note: Compile one generated crate first.",
    ] {
        assert!(human.contains(fact), "{human}");
    }
}

#[test]
fn short_invalid_path_is_redacted_without_claiming_a_resource_limit() {
    let diagnostic = CodegenDiagnostic::note("SF4017", "Cargo check completed.").with_primary(
        DiagnosticLabel::new("AUX", 0..1, 1, 1, "Invalid portable path"),
    );
    let report = DiagnosticReport::new(vec![diagnostic], false);
    assert!(!report.truncated());
    assert!(DiagnosticPolicy::default().enforce(report.clone()).is_ok());
    let value = json(&report);
    assert_eq!(value["truncated"], false);
    assert_eq!(value["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(value["diagnostics"][0]["labels"][0]["path"], "<path>");
    assert!(!value.to_string().contains("redacted-path"));
}

#[test]
fn oversized_absolute_path_is_redacted_and_keeps_the_input_limit() {
    let path = format!("/{}", "a".repeat(17_000));
    let diagnostic = CodegenDiagnostic::note("SF4017", "Cargo check completed.").with_primary(
        DiagnosticLabel::new(path, 0..1, 1, 1, "Oversized absolute path"),
    );
    let report = DiagnosticReport::new(vec![diagnostic], false);
    assert!(report.truncated());
    let value = json(&report);
    assert_eq!(value["truncated"], true);
    assert_eq!(value["diagnostics"][0]["labels"][0]["path"], "<path>");
    assert_eq!(value["diagnostics"].as_array().unwrap().len(), 2);
}

#[test]
fn all_fields_debug_and_display_redact_paths_uris_credentials_and_terminal_controls() {
    let secret = concat!(
        "path=/outside/private.rs drive=C:\\Users\\private.rs unc=\\\\server\\private.rs ",
        "file:///tmp/private.rs https://alice:private@example.com/repo ",
        "ssh://git@private.example/repo password='private value' token=private-token ",
        "Authorization: Bearer private-header\n",
        "Visible \x1b[31mColor.\x1b[0m \x1b]0;private-terminal\x07",
        "\x1b]8;;https://private.example\x1b\\link\x1b]8;;\x1b\\"
    );
    let label = DiagnosticLabel::new(secret, 0..1, 1, usize::MAX, secret);
    let diagnostic = CodegenDiagnostic::error(secret, secret)
        .with_coordinate(secret)
        .with_primary(label.clone())
        .with_secondary(label)
        .with_help(secret)
        .with_note(secret);
    let error = CodegenError::from_report(DiagnosticReport::new(vec![diagnostic], false));
    for output in [
        render_diagnostics(error.report(), DiagnosticFormat::Json),
        render_diagnostics(error.report(), DiagnosticFormat::Human),
        format!("{error:?}"),
        error.to_string(),
        CodegenError::new("SF4007", secret).to_string(),
    ] {
        for forbidden in [
            "private",
            "/outside",
            "C:\\Users",
            "server",
            "://",
            "\x1b",
            "\x07",
        ] {
            assert!(!output.contains(forbidden), "secret was not redacted");
        }
        assert!(output.contains("<redacted>"));
        assert!(output.contains("Color."));
        assert!(output.len() < 16 * 1024);
    }
}

#[test]
fn record_and_byte_limits_append_one_terminal_error() {
    for diagnostics in [
        vec![CodegenDiagnostic::note("SF4017", "notice"); 1001],
        vec![CodegenDiagnostic::note("SF4017", "repeated text ".repeat(5000)); 100],
    ] {
        let report = DiagnosticReport::new(diagnostics, false);
        assert!(report.truncated());
        let value = json(&report);
        assert_eq!(value["truncated"], true);
        let records = value["diagnostics"].as_array().unwrap();
        assert!(records.len() <= 1000);
        assert_eq!(records.last().unwrap()["code"], "SF4007");
        assert_eq!(records.last().unwrap()["severity"], "error");
        assert!(
            render_diagnostics(&report, DiagnosticFormat::Human).len()
                <= MAX_DIAGNOSTIC_REPORT_BYTES
        );
    }
}

#[test]
fn label_help_and_note_limits_are_bounded_without_proportional_rendering() {
    let mut diagnostic = CodegenDiagnostic::error("SF4007", "message");
    for _ in 0..33 {
        diagnostic = diagnostic
            .with_secondary(DiagnosticLabel::new(
                "src/lib.rs",
                0..usize::MAX,
                1,
                usize::MAX,
                "label",
            ))
            .with_help("help")
            .with_note("note");
    }
    let report = DiagnosticReport::new(vec![diagnostic], false);
    assert!(report.truncated());
    let value = json(&report);
    assert_eq!(value["truncated"], true);
    for field in ["labels", "help", "notes"] {
        assert_eq!(value["diagnostics"][0][field].as_array().unwrap().len(), 32);
    }
    assert!(render_diagnostics(&report, DiagnosticFormat::Human).len() < 4096);
}

#[test]
fn truncates_each_report_field_at_a_utf8_boundary() {
    let oversized = "repeated text ".repeat(6000);
    let diagnostics = [
        CodegenDiagnostic::error("SF4007", &oversized),
        CodegenDiagnostic::error(&oversized, "message"),
        CodegenDiagnostic::error("SF4007", "message").with_coordinate(&oversized),
        CodegenDiagnostic::error("SF4007", "message").with_help(&oversized),
        CodegenDiagnostic::error("SF4007", "message").with_note(&oversized),
        CodegenDiagnostic::error("SF4007", "message").with_primary(DiagnosticLabel::new(
            "src/lib.rs",
            0..1,
            1,
            1,
            &oversized,
        )),
    ];
    for diagnostic in diagnostics {
        let report = DiagnosticReport::new(vec![diagnostic], false);
        assert!(report.truncated());
        let value = json(&report);
        assert_eq!(value["truncated"], true);
        assert_eq!(value["diagnostics"][1]["code"], "SF4007");
        assert!(value.to_string().contains("…<truncated>"));
    }
    let diagnostic = CodegenDiagnostic::error("SF4007", "message")
        .with_secondary(DiagnosticLabel::new(&oversized, 0..1, 1, 1, "label"));
    let report = DiagnosticReport::new(vec![diagnostic], false);
    assert!(report.truncated());
    let value = json(&report);
    assert_eq!(value["diagnostics"][0]["labels"][0]["path"], "<path>");
}
