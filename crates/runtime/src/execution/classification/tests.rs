//! Explicit syntax and no-application-work fixtures for HTTP routing.

mod fixture;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use chigiri_core::{ExecutionLimits, GraphQlRequest};

use super::{SelectedHttpOperation as Kind, classify};

#[test]
fn complete_shorthand_and_named_operations_have_only_their_selected_kind() {
    for (source, requested, expected) in [
        ("{ value }", None, Kind::Query),
        ("query Q { value }", None, Kind::Query),
        ("mutation M { update }", Some("M"), Kind::Mutation),
        ("subscription S { event }", Some("S"), Kind::Subscription),
        (
            "query Q { value } subscription S { event }",
            Some("Q"),
            Kind::Query,
        ),
        (
            "query Q { value } subscription S { event }",
            Some("S"),
            Kind::Subscription,
        ),
        (
            "mutation M { update } subscription S { event }",
            Some("M"),
            Kind::Mutation,
        ),
    ] {
        assert_eq!(
            classify(source, requested, &ExecutionLimits::production()),
            expected
        );
    }
}

#[test]
fn incomplete_or_ambiguous_selection_cannot_authorize_source_setup() {
    for (source, requested) in [
        ("", None),
        ("fragment F on Query { value }", None),
        ("subscription S { event", Some("S")),
        ("query Q { value } subscription S { event }", None),
        ("subscription S { event }", Some("missing")),
        ("subscription S { event }", Some("")),
        (
            "subscription S { event } subscription S { other }",
            Some("S"),
        ),
        ("{ value } subscription S { event }", Some("S")),
        ("subscription { event } query Q { value }", Some("Q")),
        (
            "type Query { value: Int } subscription S { event }",
            Some("S"),
        ),
    ] {
        assert_eq!(
            classify(source, requested, &ExecutionLimits::production()),
            Kind::Indeterminate
        );
    }
}

#[test]
fn parser_not_keyword_search_handles_comments_strings_fragments_and_descriptions() {
    for source in [
        "# subscription Spoof { event }\nquery Real { field(arg: \"subscription\") }",
        "query Real { ...F } fragment F on Query { field(arg: \"mutation M { x }\") }",
        "\"subscription description\" query Real { value }",
        "query Real { field(arg: \"\"\"subscription Spoof { event }\"\"\") }",
    ] {
        assert_eq!(
            classify(source, Some("Real"), &ExecutionLimits::production()),
            Kind::Query
        );
    }
}

#[test]
fn parser_byte_and_nesting_limits_remain_the_schema_authority() {
    let source = "subscription S { event }";
    let exact = ExecutionLimits {
        max_document_bytes: source.len(),
        ..ExecutionLimits::production()
    };
    assert_eq!(classify(source, None, &exact), Kind::Subscription);
    let short = ExecutionLimits {
        max_document_bytes: source.len() - 1,
        ..exact
    };
    assert_eq!(classify(source, None, &short), Kind::Indeterminate);
    let shallow = ExecutionLimits {
        max_parser_nesting: 1,
        ..ExecutionLimits::production()
    };
    assert_eq!(
        classify(
            "subscription S { event { child { value } } }",
            None,
            &shallow
        ),
        Kind::Indeterminate
    );
}

#[test]
fn classifier_does_not_validate_coerce_resolve_observe_or_populate_cache() {
    let (schema, calls) = fixture::schema();
    let request = GraphQlRequest::new("query Q($value: Probe = 1) { echo(value: $value) }");
    assert!(schema.inner.cache.get(&request.query).is_none());
    for _ in 0..2 {
        assert_eq!(schema.__selected_http_operation(&request), Kind::Query);
    }
    assert_eq!(calls.scalar.load(Ordering::SeqCst), 0);
    assert_eq!(calls.resolver.load(Ordering::SeqCst), 0);
    assert_eq!(calls.lifecycle.load(Ordering::SeqCst), 0);
    assert!(schema.inner.cache.get(&request.query).is_none());
    schema.validate_operation(&request).unwrap();
    assert!(calls.scalar.load(Ordering::SeqCst) > 0);
    let cached = schema.inner.cache.get(&request.query).unwrap();
    let before_observations = calls.lifecycle.load(Ordering::SeqCst);
    let before_scalars = calls.scalar.load(Ordering::SeqCst);
    assert_eq!(schema.__selected_http_operation(&request), Kind::Query);
    assert!(Arc::ptr_eq(
        &cached,
        &schema.inner.cache.get(&request.query).unwrap()
    ));
    assert_eq!(calls.scalar.load(Ordering::SeqCst), before_scalars);
    assert_eq!(calls.lifecycle.load(Ordering::SeqCst), before_observations);
}

#[test]
fn syntactic_subscription_hint_is_not_schema_validation_or_source_support() {
    let (schema, calls) = fixture::schema();
    let request =
        GraphQlRequest::new("subscription S($value: Probe = 1) { missing(value: $value) }");
    assert_eq!(
        schema.__selected_http_operation(&request),
        Kind::Subscription
    );
    assert_eq!(calls.scalar.load(Ordering::SeqCst), 0);
    assert!(schema.inner.cache.get(&request.query).is_none());
}
