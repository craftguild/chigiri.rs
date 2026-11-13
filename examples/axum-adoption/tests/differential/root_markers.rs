use chigiri_axum_adoption::differential::{
    AllowlistEntry, ComparisonCase, DifferenceClassification, ObservedResponse, compare_corpus,
    parse_allowlist,
};
use serde_json::{Value, json};

use super::response;

#[path = "root_markers/negative.rs"]
mod negative;
#[path = "root_markers/positive.rs"]
mod positive;

fn assert_validation_rejected(pointer: &str, old: Value, new: Value) {
    let source = Value::Array(vec![entry_value("negative", pointer, &old, &new)]).to_string();
    let parse_error = parse_allowlist(&source).unwrap_err();
    assert!(parse_error.contains("do not satisfy the exact-difference contract"));

    let equal = response(json!({}));
    let entries = [allow("negative", pointer, old, new)];
    let error = compare_corpus(&[case("negative", &equal, &equal)], &entries)
        .unwrap_err()
        .to_string();
    assert!(error.contains("do not satisfy the exact-difference contract"));
}

fn assert_pass(case_id: &str, old: Value, new: Value, entries: &[AllowlistEntry]) {
    let old = response(old);
    let new = response(new);
    compare_corpus(&[case(case_id, &old, &new)], entries).unwrap();
}

fn assert_fail(case_id: &str, old: Value, new: Value, entries: &[AllowlistEntry]) {
    assert!(!compare_error(case_id, old, new, entries).is_empty());
}

fn compare_error(case_id: &str, old: Value, new: Value, entries: &[AllowlistEntry]) -> String {
    let old = response(old);
    let new = response(new);
    compare_corpus(&[case(case_id, &old, &new)], entries)
        .unwrap_err()
        .to_string()
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

fn parsed(case_id: &str, entries: &[(&str, Value, Value)]) -> Vec<AllowlistEntry> {
    let entries = entries
        .iter()
        .map(|(pointer, old, new)| entry_value(case_id, pointer, old, new))
        .collect::<Vec<_>>();
    parse_allowlist(&Value::Array(entries).to_string()).unwrap()
}

fn entry_value(case_id: &str, pointer: &str, old: &Value, new: &Value) -> Value {
    json!({
        "caseId": case_id,
        "pointer": pointer,
        "old": old,
        "new": new,
        "reason": "The structural difference was reviewed.",
        "owner": "migration",
        "classification": "intended-product-change"
    })
}

fn allow(case_id: &str, pointer: &str, old: Value, new: Value) -> AllowlistEntry {
    AllowlistEntry {
        case_id: case_id.to_owned(),
        pointer: pointer.to_owned(),
        old,
        new,
        reason: "The structural difference was reviewed.".to_owned(),
        owner: "migration".to_owned(),
        classification: DifferenceClassification::IntendedProductChange,
    }
}

fn marker(kind: &str) -> Value {
    json!({"$chigiriDifference": kind})
}
