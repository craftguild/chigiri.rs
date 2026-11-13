#[path = "support.rs"]
mod support;

use chigiri_runtime::GraphQlRequest;

#[test]
fn op_01_calculates_a_stable_digest_from_exact_document_bytes() {
    let schema = support::schema();
    let first = schema
        .validate_operation(&GraphQlRequest::new("query One { value }"))
        .unwrap();
    let repeated = schema
        .validate_operation(&GraphQlRequest::new("query One { value }"))
        .unwrap();
    let changed = schema
        .validate_operation(&GraphQlRequest::new("query One {  value }"))
        .unwrap();
    assert_eq!(first, repeated);
    assert_ne!(
        first.key().document_digest(),
        changed.key().document_digest()
    );
}

#[test]
fn op_02_multiple_operations_require_a_selection_name_for_distinct_keys() {
    let schema = support::schema();
    let document = "query One { value } query Two { other }";
    let error = schema
        .validate_operation(&GraphQlRequest::new(document))
        .unwrap_err();
    assert_eq!(
        error.errors[0]
            .extensions
            .get("code")
            .and_then(|value| value.as_str()),
        Some("OPERATION_NOT_FOUND")
    );
    let one = schema
        .validate_operation(&GraphQlRequest::new(document).with_operation_name("One"))
        .unwrap();
    let two = schema
        .validate_operation(&GraphQlRequest::new(document).with_operation_name("Two"))
        .unwrap();
    assert_ne!(one.key(), two.key());
}
