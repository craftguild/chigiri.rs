use chigiri_axum_adoption::differential::{
    AllowlistEntry, ComparisonCase, DifferenceClassification, compare_corpus, parse_allowlist,
};

use super::response;

#[test]
fn comparator_ignores_object_key_order_but_preserves_array_order_and_field_presence() {
    let old = response(serde_json::json!({"data": {"a": 1, "b": 2}}));
    let reordered = response(serde_json::json!({"data": {"b": 2, "a": 1}}));
    compare_corpus(&[case("order", &old, &reordered)], &[]).unwrap();

    let array_old = response(serde_json::json!({"data": [1, 2]}));
    let array_new = response(serde_json::json!({"data": [2, 1]}));
    assert!(compare_corpus(&[case("array", &array_old, &array_new)], &[]).is_err());

    let absent = response(serde_json::json!({"data": {}}));
    let explicit_null = response(serde_json::json!({"data": {"value": null}}));
    assert!(compare_corpus(&[case("presence", &absent, &explicit_null)], &[]).is_err());
}

#[test]
fn array_length_differences_require_allowlisting_each_missing_index() {
    let old = response(serde_json::json!({"data": [1]}));
    let new = response(serde_json::json!({"data": [1, 2, 3]}));
    let entries = parse_allowlist(
        r#"[
          {"caseId":"growth","pointer":"/body/data/1","old":{"$chigiriDifference":"missing"},"new":2,"reason":"Add the second item.","owner":"migration","classification":"intended-product-change"},
          {"caseId":"growth","pointer":"/body/data/2","old":{"$chigiriDifference":"missing"},"new":3,"reason":"Add the third one.","owner":"migration","classification":"intended-product-change"}
        ]"#,
    )
    .unwrap();
    compare_corpus(&[case("growth", &old, &new)], &entries).unwrap();
    assert!(compare_corpus(&[case("growth", &old, &new)], &entries[..1]).is_err());

    let shrink = parse_allowlist(
        r#"[
          {"caseId":"shrink","pointer":"/body/data/1","old":2,"new":{"$chigiriDifference":"missing"},"reason":"Remove the second item.","owner":"migration","classification":"intended-product-change"},
          {"caseId":"shrink","pointer":"/body/data/2","old":3,"new":{"$chigiriDifference":"missing"},"reason":"Remove the third item.","owner":"migration","classification":"intended-product-change"}
        ]"#,
    )
    .unwrap();
    compare_corpus(&[case("shrink", &new, &old)], &shrink).unwrap();
}

#[test]
fn container_differences_require_marker_and_descendant_entries() {
    let old = response(serde_json::json!({"data": {"value": {"nested": 1}}}));
    let new = response(serde_json::json!({"data": {"value": null}}));
    let entries = parse_allowlist(
        r#"[
          {"caseId":"shape","pointer":"/body/data/value","old":{"$chigiriDifference":"object"},"new":null,"reason":"Change of type","owner":"migration","classification":"intended-product-change"},
          {"caseId":"shape","pointer":"/body/data/value/nested","old":1,"new":{"$chigiriDifference":"missing"},"reason":"Remove the field","owner":"migration","classification":"intended-product-change"}
        ]"#,
    )
    .unwrap();
    compare_corpus(&[case("shape", &old, &new)], &entries).unwrap();
    assert!(compare_corpus(&[case("shape", &old, &new)], &entries[..1]).is_err());

    let object = response(serde_json::json!({"data": {"value": {"0": 1}}}));
    let array = response(serde_json::json!({"data": {"value": [2]}}));
    let transition = parse_allowlist(
        r#"[
          {"caseId":"kind","pointer":"/body/data/value","old":{"$chigiriDifference":"object"},"new":{"$chigiriDifference":"array"},"reason":"Change of container type","owner":"migration","classification":"intended-product-change"},
          {"caseId":"kind","pointer":"/body/data/value/0","old":1,"new":2,"reason":"Change the value of the same segment.","owner":"migration","classification":"intended-product-change"}
        ]"#,
    )
    .unwrap();
    compare_corpus(&[case("kind", &object, &array)], &transition).unwrap();
}

#[test]
fn rejects_snapshot_pointers_and_container_values_in_the_parser() {
    for pointer in [
        "/body",
        "/body/data",
        "/body/errors",
        "/body/data/*",
        "/body/data/value~2key",
        "/headers/x",
    ] {
        assert!(parse_allowlist(&entry_json(pointer, "1", "2")).is_err());
    }
    assert!(parse_allowlist(&entry_json("/body/data/value", r#"{"nested":1}"#, "null")).is_err());
    assert!(parse_allowlist(&entry_json("/body/data/value", "[1]", "[2]")).is_err());
    assert!(parse_allowlist(&entry_json("/body/data/value", "null", r#"{"nested":1}"#)).is_err());
    assert!(parse_allowlist(&entry_json("/body/data/value", "null", "[2]")).is_err());
    assert!(
        parse_allowlist(&entry_json(
            "/body/data/value",
            r#"{"$chigiriDifference":"unknown"}"#,
            "2"
        ))
        .is_err()
    );
    assert!(
        parse_allowlist(&entry_json(
            "/body/data/value",
            r#"{"$chigiriDifference":"missing","snapshot":1}"#,
            "2"
        ))
        .is_err()
    );
    let unknown =
        entry_json("/body/data/value", "1", "2").replace("intended-product-change", "unknown");
    assert!(parse_allowlist(&unknown).is_err());
}

#[test]
fn compare_input_revalidates_allowlist_entries_directly() {
    let old = response(serde_json::json!({"data": {"value": 1}}));
    let new = response(serde_json::json!({"data": {"value": 2}}));
    let comparison = [case("direct", &old, &new)];
    let valid = allow("direct", "/body/data/value", 1.into(), 2.into());

    let mut invalid = valid.clone();
    invalid.pointer = "/body".to_owned();
    assert!(compare_corpus(&comparison, &[invalid]).is_err());
    let mut invalid = valid.clone();
    invalid.reason = " ".to_owned();
    assert!(compare_corpus(&comparison, &[invalid]).is_err());
    let mut invalid = valid.clone();
    invalid.owner.clear();
    assert!(compare_corpus(&comparison, &[invalid]).is_err());
    let mut invalid = valid.clone();
    invalid.case_id = " ".to_owned();
    assert!(compare_corpus(&comparison, &[invalid]).is_err());
    for (old, new) in [
        (serde_json::json!({"snapshot": 1}), serde_json::json!(2)),
        (serde_json::json!([1]), serde_json::json!(2)),
        (serde_json::json!(1), serde_json::json!({"snapshot": 2})),
        (serde_json::json!(1), serde_json::json!([2])),
    ] {
        let mut invalid = valid.clone();
        invalid.old = old;
        invalid.new = new;
        assert!(compare_corpus(&comparison, &[invalid]).is_err());
    }
    let mut invalid = valid;
    invalid.classification = DifferenceClassification::Regression;
    assert!(compare_corpus(&comparison, &[invalid]).is_err());
}

#[test]
fn rejects_duplicate_case_ids_and_unknown_allowlist_cases() {
    let response = response(serde_json::json!({"data": {"value": 1}}));
    let duplicate = [
        case("same", &response, &response),
        case("same", &response, &response),
    ];
    assert!(compare_corpus(&duplicate, &[]).is_err());

    let unknown = allow("missing", "/body/data/value", 1.into(), 2.into());
    assert!(compare_corpus(&[case("known", &response, &response)], &[unknown]).is_err());

    assert!(compare_corpus(&[case("", &response, &response)], &[]).is_err());

    let duplicate_entry = allow("known", "/body/data/value", 1.into(), 2.into());
    assert!(
        compare_corpus(
            &[case("known", &response, &response)],
            &[duplicate_entry.clone(), duplicate_entry],
        )
        .is_err()
    );

    let unused = allow("known", "/body/data/value", 1.into(), 2.into());
    assert!(compare_corpus(&[case("known", &response, &response)], &[unused]).is_err());
}

#[test]
fn corpus_reader_rejects_duplicate_case_ids() {
    let source = r#"[
      {"id":"same","query":"{ value }","headers":{},"compareHeaders":[],"legacy":{"status":200,"mediaType":"application/graphql-response+json; charset=utf-8","headers":{},"body":{}}},
      {"id":"same","query":"{ value }","headers":{},"compareHeaders":[],"legacy":{"status":200,"mediaType":"application/graphql-response+json; charset=utf-8","headers":{},"body":{}}}
    ]"#;
    assert!(std::panic::catch_unwind(|| super::support::load_cases(source)).is_err());
}

#[test]
fn one_allowlist_entry_cannot_match_two_differences() {
    let mut old = response(serde_json::json!({"data": {"value": 1}}));
    let mut new = response(serde_json::json!({"data": {"value": 1}}));
    old.headers
        .insert("x-contract".to_owned(), vec!["old".to_owned()]);
    new.headers
        .insert("x-contract".to_owned(), vec!["new".to_owned()]);
    let names = ["x-contract".to_owned(), "x-contract".to_owned()];
    let comparison = ComparisonCase {
        case_id: "headers",
        old: &old,
        new: &new,
        header_names: &names,
        volatile_extension_pointers: &[],
    };
    let entry = allow(
        "headers",
        "/headers/x-contract/0",
        "old".into(),
        "new".into(),
    );
    assert!(compare_corpus(&[comparison], &[entry]).is_err());
}

#[test]
fn volatile_extension_pointers_require_numeric_error_indices_and_the_extensions_path() {
    let old = response(serde_json::json!({
        "errors": [{"extensions": {"code": "FAIL", "traceId": "old"}}]
    }));
    let new = response(serde_json::json!({
        "errors": [{"extensions": {"code": "FAIL", "traceId": "new"}}]
    }));
    let pointer = "/body/errors/0/extensions/traceId".to_owned();
    let pointers = [pointer];
    compare_corpus(
        &[case_with_volatile("volatile", &old, &new, &pointers)],
        &[],
    )
    .unwrap();

    for invalid in [
        "/body/errors/x/extensions/traceId",
        "/body/errors/01/extensions/traceId",
        "/body/errors/0/nested/extensions/traceId",
        "/body/errors/0/extensions",
        "/body/errors/0/extensions/traceId/*",
    ] {
        let pointers = [invalid.to_owned()];
        assert!(
            compare_corpus(
                &[case_with_volatile("volatile", &old, &new, &pointers)],
                &[],
            )
            .is_err()
        );
    }

    let duplicate = "/body/errors/0/extensions/traceId".to_owned();
    let pointers = [duplicate.clone(), duplicate];
    assert!(
        compare_corpus(
            &[case_with_volatile("volatile", &old, &new, &pointers)],
            &[],
        )
        .is_err()
    );
}

#[test]
fn volatile_extensions_do_not_consume_pointers_without_real_differences() {
    let equal = response(serde_json::json!({
        "errors": [{"extensions": {"code": "FAIL", "traceId": "same"}}]
    }));
    for pointer in [
        "/body/errors/0/extensions/traceId",
        "/body/errors/0/extensions/absent",
    ] {
        let pointers = [pointer.to_owned()];
        assert!(
            compare_corpus(
                &[case_with_volatile("unused", &equal, &equal, &pointers)],
                &[],
            )
            .is_err()
        );
    }
}

#[test]
fn volatile_extension_pointers_allow_one_sided_presence_differences() {
    let present = response(serde_json::json!({
        "errors": [{"extensions": {"code": "FAIL", "traceId": "value"}}]
    }));
    let absent = response(serde_json::json!({
        "errors": [{"extensions": {"code": "FAIL"}}]
    }));
    let pointers = ["/body/errors/0/extensions/traceId".to_owned()];
    compare_corpus(
        &[case_with_volatile("remove", &present, &absent, &pointers)],
        &[],
    )
    .unwrap();
    compare_corpus(
        &[case_with_volatile("add", &absent, &present, &pointers)],
        &[],
    )
    .unwrap();
}

fn case<'a>(
    case_id: &'a str,
    old: &'a chigiri_axum_adoption::differential::ObservedResponse,
    new: &'a chigiri_axum_adoption::differential::ObservedResponse,
) -> ComparisonCase<'a> {
    ComparisonCase {
        case_id,
        old,
        new,
        header_names: &[],
        volatile_extension_pointers: &[],
    }
}

fn case_with_volatile<'a>(
    case_id: &'a str,
    old: &'a chigiri_axum_adoption::differential::ObservedResponse,
    new: &'a chigiri_axum_adoption::differential::ObservedResponse,
    pointers: &'a [String],
) -> ComparisonCase<'a> {
    ComparisonCase {
        case_id,
        old,
        new,
        header_names: &[],
        volatile_extension_pointers: pointers,
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
        owner: "migration".to_owned(),
        classification: DifferenceClassification::IntendedProductChange,
    }
}

fn entry_json(pointer: &str, old: &str, new: &str) -> String {
    format!(
        r#"[{{"caseId":"x","pointer":"{pointer}","old":{old},"new":{new},"reason":"r","owner":"o","classification":"intended-product-change"}}]"#
    )
}
