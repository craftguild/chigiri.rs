use super::*;

#[test]
fn root_09_rejects_scalar_transitions_at_the_data_root() {
    assert_validation_rejected("/body/data", json!(1), json!(2));
    assert_validation_rejected("/body/data", Value::Null, json!(2));
}

#[test]
fn root_10_rejects_container_markers_at_the_body_root() {
    assert_validation_rejected("/body", marker("object"), marker("array"));
}

#[test]
fn root_11_rejects_markers_on_custom_body_members() {
    assert_validation_rejected("/body/custom", marker("missing"), marker("object"));
}

#[test]
fn root_12_raw_object_and_array_snapshot_are_rejected_at_both_entries() {
    assert_validation_rejected("/body/data", marker("object"), json!({"raw": 1}));
    assert_validation_rejected("/body/data", marker("array"), json!([1]));
}

#[test]
fn root_13_rejects_noncanonical_markers_and_pointers_on_either_side() {
    assert_validation_rejected("/body/data", marker("unknown"), Value::Null);
    assert_validation_rejected(
        "/body/data",
        json!({"$chigiriDifference": "object", "extra": true}),
        Value::Null,
    );
    for pointer in [
        "/body/data/*",
        "/body/data/",
        "/body//value",
        "/body/data~2value",
    ] {
        assert_validation_rejected(pointer, marker("object"), Value::Null);
    }
}

#[test]
fn root_14_rejects_unused_extra_marker_and_leaf_entries() {
    for extra in [
        allow(
            "root-14",
            "/body/data/extra",
            marker("missing"),
            marker("object"),
        ),
        allow("root-14", "/body/data/extra", marker("missing"), json!(1)),
    ] {
        let entries = [
            allow("root-14", "/body/data", Value::Null, marker("object")),
            allow("root-14", "/body/data/value", marker("missing"), json!(1)),
            extra,
        ];
        let error = compare_error(
            "root-14",
            json!({"data": null}),
            json!({"data": {"value": 1}}),
            &entries,
        );
        assert!(error.contains("unused allowlist entry"));
    }
}

#[test]
fn root_15_duplicate_unknown_case_regression() {
    let entry = allow("root-15", "/body/data", Value::Null, marker("object"));
    let old = response(json!({"data": null}));
    let new = response(json!({"data": {}}));
    let comparison = [case("root-15", &old, &new)];
    assert!(compare_corpus(&comparison, &[entry.clone(), entry.clone()]).is_err());

    let duplicate_source = Value::Array(vec![
        entry_value("root-15", "/body/data", &Value::Null, &marker("object")),
        entry_value("root-15", "/body/data", &Value::Null, &marker("object")),
    ]);
    assert!(parse_allowlist(&duplicate_source.to_string()).is_err());

    let mut unknown = entry.clone();
    unknown.case_id = "unknown".to_owned();
    assert!(compare_corpus(&comparison, &[unknown]).is_err());

    let mut regression = entry;
    regression.classification = DifferenceClassification::Regression;
    assert!(compare_corpus(&comparison, &[regression]).is_err());
}
