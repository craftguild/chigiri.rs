use chigiri_testkit::differential::{
    AllowlistEntry, ComparisonCase, DifferenceClassification, ObservedResponse, compare_corpus,
    parse_allowlist,
};

#[test]
fn strict_allowlist_negative_suite() {
    let ordered = response(serde_json::json!({"data": {"a": 1, "b": 2}}));
    let reordered = response(serde_json::json!({"data": {"b": 2, "a": 1}}));
    compare_corpus(&[case("object-order", &ordered, &reordered)], &[]).unwrap();

    let array_old = response(serde_json::json!({"data": [1, 2]}));
    let array_new = response(serde_json::json!({"data": [2, 1]}));
    assert!(compare_corpus(&[case("array-order", &array_old, &array_new)], &[]).is_err());

    let missing = response(serde_json::json!({"data": {}}));
    let explicit_null = response(serde_json::json!({"data": {"value": null}}));
    assert!(compare_corpus(&[case("presence", &missing, &explicit_null)], &[]).is_err());

    let difference_old = response(serde_json::json!({"data": {"value": 1}}));
    let difference_new = response(serde_json::json!({"data": {"value": 2}}));
    let entry = allow("exact", "/body/data/value", 1.into(), 2.into());
    compare_corpus(
        &[case("exact", &difference_old, &difference_new)],
        std::slice::from_ref(&entry),
    )
    .unwrap();
    assert!(compare_corpus(&[case("other", &difference_old, &difference_old)], &[entry]).is_err());

    let regression = AllowlistEntry {
        classification: DifferenceClassification::Regression,
        ..allow("exact", "/body/data/value", 1.into(), 2.into())
    };
    assert!(
        compare_corpus(
            &[case("exact", &difference_old, &difference_new)],
            &[regression]
        )
        .is_err()
    );

    assert!(parse_allowlist(
        r#"[{"caseId":"x","pointer":"/body/*","old":1,"new":2,"reason":"r","owner":"o","classification":"specification-fix"}]"#,
    )
    .is_err());
}

fn response(body: serde_json::Value) -> ObservedResponse {
    ObservedResponse {
        status: 200,
        media_type: "application/graphql-response+json; charset=utf-8".to_owned(),
        headers: Default::default(),
        body,
    }
}

fn case<'a>(
    case_id: &'a str,
    old: &'a ObservedResponse,
    new: &'a ObservedResponse,
) -> ComparisonCase<'a> {
    ComparisonCase {
        case_id,
        old,
        new,
        header_names: &[],
        volatile_extension_pointers: &[],
    }
}

fn allow(
    case_id: &str,
    pointer: &str,
    old: serde_json::Value,
    new: serde_json::Value,
) -> AllowlistEntry {
    AllowlistEntry {
        case_id: case_id.to_owned(),
        pointer: pointer.to_owned(),
        old,
        new,
        reason: "individual differences verified".to_owned(),
        owner: "testkit".to_owned(),
        classification: DifferenceClassification::SpecificationFix,
    }
}
