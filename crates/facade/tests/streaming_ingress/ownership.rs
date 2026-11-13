use std::sync::atomic::Ordering;

use chigiri::__streaming::SubscriptionHttpIngress;
use chigiri::spi::RequestHeader;
use chigiri::{LifecyclePhase, LifecycleResult, ResponseParts};

use super::support::{RequestInput, TrackingDeadlines, harness, invoke, json_headers};

const SUBSCRIPTION: &str = "subscription S { event }";

fn assert_safe_rejection(response: &ResponseParts, status: u16, message: &str, code: &str) {
    assert_eq!(response.status, status);
    assert_eq!(response.headers.len(), 1);
    assert_eq!(response.headers[0].name, "content-type");
    assert_eq!(
        response.headers[0].value,
        "application/graphql-response+json; charset=utf-8"
    );
    assert_eq!(
        response.body,
        format!(r#"{{"errors":[{{"message":"{message}","extensions":{{"code":"{code}"}}}}]}}"#)
            .as_bytes()
    );
    assert!(
        !response
            .headers
            .iter()
            .any(|header| matches!(header.name.as_str(), "allow" | "retry-after"))
    );
}

#[test]
fn endpoint_clones_share_one_pending_capacity_and_drop_restores_it() {
    let harness = harness(16 * 1024, 1, SUBSCRIPTION);
    let clone = harness.endpoint.clone();
    let first_deadlines = TrackingDeadlines::default();
    let headers = json_headers(b"text/event-stream");
    let body = br#"{"query":"subscription S { event }"}"#.to_vec();
    let (first, _) = invoke(
        &harness.endpoint,
        &first_deadlines,
        RequestInput {
            method: "POST",
            uri_query: None,
            headers: &headers,
            body: body.clone(),
        },
    );
    let SubscriptionHttpIngress::Pending(first) = first else {
        panic!("first subscription did not acquire pending capacity");
    };
    assert_eq!(harness.signals.admission.load(Ordering::SeqCst), 1);
    assert_eq!(harness.signals.source.load(Ordering::SeqCst), 1);
    assert_eq!(harness.signals.decoded.load(Ordering::SeqCst), 1);

    let second_deadlines = TrackingDeadlines::default();
    let (second, _) = invoke(
        &clone,
        &second_deadlines,
        RequestInput {
            method: "POST",
            uri_query: None,
            headers: &headers,
            body: body.clone(),
        },
    );
    let SubscriptionHttpIngress::Finite(second) = second else {
        panic!("shared pending capacity admitted a second subscription");
    };
    assert_safe_rejection(
        &second,
        503,
        "Subscription capacity is unavailable",
        "SUBSCRIPTION_LIMIT_EXCEEDED",
    );
    assert_eq!(harness.endpoint.in_flight_requests(), 1);
    assert_eq!(harness.signals.admission.load(Ordering::SeqCst), 2);
    assert_eq!(harness.signals.source.load(Ordering::SeqCst), 2);
    assert_eq!(harness.signals.decoded.load(Ordering::SeqCst), 2);
    assert_eq!(
        harness.signals.count_code(
            LifecyclePhase::RequestCompleted,
            LifecycleResult::Rejected,
            Some("SUBSCRIPTION_LIMIT_EXCEEDED"),
        ),
        1
    );
    assert_eq!(
        harness
            .signals
            .count(LifecyclePhase::RequestCompleted, LifecycleResult::Cancelled,),
        0
    );
    assert_eq!(harness.signals.context.load(Ordering::SeqCst), 0);
    assert_eq!(harness.signals.scalar.load(Ordering::SeqCst), 0);
    assert_eq!(harness.signals.resolver.load(Ordering::SeqCst), 0);

    drop(first);
    assert_eq!(harness.endpoint.in_flight_requests(), 0);

    let third_deadlines = TrackingDeadlines::default();
    let (third, _) = invoke(
        &clone,
        &third_deadlines,
        RequestInput {
            method: "POST",
            uri_query: None,
            headers: &headers,
            body,
        },
    );
    let SubscriptionHttpIngress::Pending(third) = third else {
        panic!("dropping the pending owner did not restore capacity");
    };
    assert_eq!(harness.endpoint.in_flight_requests(), 1);
    drop(third);
    assert_eq!(harness.endpoint.in_flight_requests(), 0);
    assert_eq!(harness.signals.context.load(Ordering::SeqCst), 0);
    assert_eq!(harness.signals.scalar.load(Ordering::SeqCst), 0);
    assert_eq!(harness.signals.resolver.load(Ordering::SeqCst), 0);
}

#[test]
fn retained_metadata_rejection_releases_pending_capacity() {
    let harness = harness(2 * 1024, 1, SUBSCRIPTION);
    let deadlines = TrackingDeadlines::default();
    let oversized_value = vec![b'x'; 4 * 1024];
    let headers = [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("accept", b"text/event-stream"),
        RequestHeader::new("x-retained-metadata", &oversized_value),
    ];
    let body = br#"{"query":"subscription S { event }"}"#.to_vec();
    let (rejected, _) = invoke(
        &harness.endpoint,
        &deadlines,
        RequestInput {
            method: "POST",
            uri_query: None,
            headers: &headers,
            body: body.clone(),
        },
    );
    let SubscriptionHttpIngress::Finite(rejected) = rejected else {
        panic!("oversized retained metadata entered pending state");
    };
    assert_safe_rejection(
        &rejected,
        413,
        "Subscriber request metadata exceeds its limit",
        "BAD_USER_INPUT",
    );
    assert_eq!(harness.endpoint.in_flight_requests(), 0);
    assert_eq!(
        harness.signals.count_code(
            LifecyclePhase::RequestCompleted,
            LifecycleResult::Rejected,
            Some("BAD_USER_INPUT"),
        ),
        1
    );
    assert_eq!(
        harness
            .signals
            .count(LifecyclePhase::RequestCompleted, LifecycleResult::Cancelled,),
        0
    );
    assert_eq!(harness.signals.context.load(Ordering::SeqCst), 0);
    assert_eq!(harness.signals.scalar.load(Ordering::SeqCst), 0);
    assert_eq!(harness.signals.resolver.load(Ordering::SeqCst), 0);
    assert_eq!(harness.signals.admission.load(Ordering::SeqCst), 1);
    assert_eq!(harness.signals.source.load(Ordering::SeqCst), 1);
    assert_eq!(harness.signals.decoded.load(Ordering::SeqCst), 1);

    let clean_deadlines = TrackingDeadlines::default();
    let clean_headers = json_headers(b"text/event-stream");
    let (clean, _) = invoke(
        &harness.endpoint,
        &clean_deadlines,
        RequestInput {
            method: "POST",
            uri_query: None,
            headers: &clean_headers,
            body,
        },
    );
    let SubscriptionHttpIngress::Pending(clean) = clean else {
        panic!("metadata rejection leaked pending capacity");
    };
    assert_eq!(harness.endpoint.in_flight_requests(), 1);
    drop(clean);
    assert_eq!(harness.endpoint.in_flight_requests(), 0);
    assert_eq!(harness.signals.admission.load(Ordering::SeqCst), 2);
    assert_eq!(harness.signals.source.load(Ordering::SeqCst), 2);
    assert_eq!(harness.signals.decoded.load(Ordering::SeqCst), 2);
}
