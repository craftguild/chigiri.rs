//! Stream-only request errors retain negotiation without authorizing a source.

use std::sync::atomic::Ordering;

use chigiri::__streaming::SubscriptionHttpIngress;

use super::support::{RequestInput, TrackingDeadlines, harness, invoke, json_headers};

const SUBSCRIPTION: &str = "subscription S { event }";
// Existing GraphQL-response+JSON sizing/status contract: parse 400, validation 422.
const CASES: [(&str, Option<&str>, u16); 6] = [
    ("subscription {", None, 400),
    ("fragment F on Query { echo }", None, 422),
    (
        "subscription A { event } subscription B { event }",
        None,
        422,
    ),
    ("subscription S { event }", Some("missing"), 422),
    ("subscription S { event }", Some(""), 422),
    ("{ echo } subscription S { event }", Some("S"), 422),
];

fn body(source: &str, operation: Option<&str>) -> Vec<u8> {
    match operation {
        Some(operation) => format!(r#"{{"query":"{source}","operationName":"{operation}"}}"#),
        None => format!(r#"{{"query":"{source}"}}"#),
    }
    .into_bytes()
}

#[test]
fn stream_only_request_errors_enter_bounded_preparation_without_source_authorization() {
    for (source, operation, _) in CASES {
        for accept in [
            b"text/event-stream".as_slice(),
            b"multipart/mixed;subscriptionSpec=1.0",
        ] {
            let harness = harness(16 * 1024, 1, SUBSCRIPTION);
            let deadlines = TrackingDeadlines::default();
            let headers = json_headers(accept);
            let (outcome, _) = invoke(
                &harness.endpoint,
                &deadlines,
                RequestInput {
                    method: "POST",
                    uri_query: None,
                    headers: &headers,
                    body: body(source, operation),
                },
            );
            let SubscriptionHttpIngress::Pending(pending) = outcome else {
                panic!("explicit stream-only request error lost the selected profile");
            };
            assert_eq!(harness.signals.context.load(Ordering::SeqCst), 0);
            assert_eq!(harness.signals.scalar.load(Ordering::SeqCst), 0);
            assert_eq!(harness.signals.resolver.load(Ordering::SeqCst), 0);
            drop(pending);
            assert_eq!(harness.endpoint.in_flight_requests(), 0);
        }
    }
}

#[test]
fn indeterminate_selection_prefers_the_existing_finite_error_when_acceptable() {
    for (source, operation, expected_status) in CASES {
        let harness = harness(16 * 1024, 1, SUBSCRIPTION);
        let deadlines = TrackingDeadlines::default();
        let headers = json_headers(b"text/event-stream, application/graphql-response+json");
        let (outcome, _) = invoke(
            &harness.endpoint,
            &deadlines,
            RequestInput {
                method: "POST",
                uri_query: None,
                headers: &headers,
                body: body(source, operation),
            },
        );
        let SubscriptionHttpIngress::Finite(response) = outcome else {
            panic!("indeterminate selection replaced an acceptable finite response");
        };
        assert_eq!(response.status, expected_status);
        assert!(response.headers.iter().any(|header| {
            header.name == "content-type"
                && header
                    .value
                    .starts_with("application/graphql-response+json")
        }));
        assert_eq!(harness.endpoint.in_flight_requests(), 0);
    }
}

#[test]
fn stream_only_request_errors_use_the_same_pending_capacity() {
    let harness = harness(16 * 1024, 1, SUBSCRIPTION);
    let deadlines = TrackingDeadlines::default();
    let headers = json_headers(b"text/event-stream");
    let invoke_error = || {
        invoke(
            &harness.endpoint,
            &deadlines,
            RequestInput {
                method: "POST",
                uri_query: None,
                headers: &headers,
                body: body("subscription {", None),
            },
        )
        .0
    };
    let first = invoke_error();
    assert!(matches!(first, SubscriptionHttpIngress::Pending(_)));
    let SubscriptionHttpIngress::Finite(response) = invoke_error() else {
        panic!("request-error preparation bypassed pending capacity");
    };
    assert_eq!(response.status, 503);
    assert!(
        std::str::from_utf8(&response.body)
            .unwrap()
            .contains("SUBSCRIPTION_LIMIT_EXCEEDED")
    );
    assert_eq!(harness.signals.context.load(Ordering::SeqCst), 0);
    drop(first);
    assert!(matches!(
        invoke_error(),
        SubscriptionHttpIngress::Pending(_)
    ));
    assert_eq!(harness.endpoint.in_flight_requests(), 0);
}
