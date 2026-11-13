use super::*;

fn diagnostic() -> ReportDiagnostic {
    ReportDiagnostic {
        severity: "error".into(),
        file: None,
        coordinate: Some("Query.viewer".into()),
        member: None,
        line: None,
        column: None,
        embedded: Some(ReportPosition { line: 2, column: 3 }),
        phase: "business".into(),
        cause: "MOCK_TYPE_MISMATCH".into(),
        expected: "string".into(),
        actual: "number".into(),
        hint: "Return a string.".into(),
        message: "resolver result does not match its declared type".into(),
        references: vec![ReportReference {
            topic: "resolvers".into(),
            name: "Query.viewer".into(),
        }],
        navigation_hook: None,
        navigation_consumer: None,
    }
}

fn fixture() -> CheckReport {
    let mut report = CheckReport::new("targets", false, false);
    report.targets = vec!["resolvers/Query/viewer.luau".into()];
    report.diagnostics = vec![diagnostic()];
    report
}

#[test]
fn v3_fixture_is_exact_and_round_trips() {
    let body = encode(&fixture()).unwrap();
    let expected = concat!(
        "{\"format\":\"chigiri-mock-check/v3\",\"scope\":\"targets\",",
        "\"valid\":false,\"readiness_checked\":false,",
        "\"targets\":[\"resolvers/Query/viewer.luau\"],\"diagnostics\":[{",
        "\"severity\":\"error\",\"file\":null,\"coordinate\":\"Query.viewer\",",
        "\"member\":null,\"line\":null,\"column\":null,",
        "\"embedded\":{\"line\":2,\"column\":3},\"phase\":\"business\",",
        "\"cause\":\"MOCK_TYPE_MISMATCH\",\"expected\":\"string\",",
        "\"actual\":\"number\",\"hint\":\"Return a string.\",",
        "\"message\":\"resolver result does not match its declared type\",",
        "\"references\":[{\"topic\":\"resolvers\",\"name\":\"Query.viewer\"}]}]}\n"
    );
    assert_eq!(String::from_utf8(body).unwrap(), expected);
    let decoded: CheckReport = serde_json::from_str(expected).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded, fixture());
}

#[test]
fn reader_rejects_legacy_missing_extra_and_wrong_types() {
    let value = serde_json::to_value(fixture()).unwrap();
    for changed in [
        change(&value, |object| {
            object.insert("format".into(), "chigiri-mock-check/v2".into())
        }),
        change(&value, |object| object.remove("scope")),
        change(&value, |object| {
            object.insert("checked_targets".into(), Vec::<String>::new().into())
        }),
        change(&value, |object| {
            object.insert("valid".into(), "true".into())
        }),
    ] {
        if let Ok(report) = serde_json::from_value::<CheckReport>(changed) {
            assert!(report.validate().is_err());
        }
    }
}

#[test]
fn semantic_validation_rejects_unpaired_positions_and_bad_references() {
    let mut report = fixture();
    report.diagnostics[0].line = Some(1);
    assert!(report.validate().is_err());

    let mut report = fixture();
    report.diagnostics[0].references[0].topic = "all".into();
    assert!(report.validate().is_err());

    let mut report = fixture();
    report.diagnostics[0].references = (0..5)
        .map(|index| ReportReference {
            topic: "runtime".into(),
            name: format!("item-{index}"),
        })
        .collect();
    assert!(report.validate().is_err());

    let mut report = fixture();
    report.readiness_checked = true;
    assert!(report.validate().is_err());
}

#[test]
fn output_limit_fails_instead_of_truncating_a_valid_report() {
    let mut report = fixture();
    report.diagnostics[0].message = "x".repeat(MAX_OUTPUT_BYTES);
    let error = encode(&report).unwrap_err();
    assert_eq!(error.message, "check output exceeds 16 MiB");
}

#[test]
fn location_fields_keep_independent_provenance() {
    let mut bundle = Diagnostic::new(
        std::path::Path::new("bundle.toml"),
        "parse",
        "invalid closed TOML",
    );
    bundle.set_position((7, 9));
    let bundle = ReportDiagnostic::from_error(&bundle);
    assert_eq!(bundle.file.as_deref(), Some("bundle.toml"));
    assert_eq!((bundle.line, bundle.column), (Some(7), Some(9)));
    assert!(bundle.coordinate.is_none());
}

fn change(
    value: &serde_json::Value,
    update: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>) -> Option<serde_json::Value>,
) -> serde_json::Value {
    let mut changed = value.clone();
    let _ = update(changed.as_object_mut().unwrap());
    changed
}
