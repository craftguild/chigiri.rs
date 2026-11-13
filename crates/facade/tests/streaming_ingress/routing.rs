use std::sync::atomic::Ordering;

use chigiri::__streaming::SubscriptionHttpIngress;
use chigiri::spi::RequestHeader;
use chigiri::{LifecyclePhase, LifecycleResult, SubscriptionTransport};

use super::support::{RequestInput, SETUP, TrackingDeadlines, harness, invoke, json_headers};

const SUBSCRIPTION: &str = "subscription S($value: Probe = 1) { event(value: $value) }";

#[test]
fn selected_subscription_returns_bounded_pending_without_application_execution() {
    for (accept, expected) in [
        (
            b"text/event-stream".as_slice(),
            SubscriptionTransport::ServerSentEvents,
        ),
        (
            b"multipart/mixed;subscriptionSpec=1.0".as_slice(),
            SubscriptionTransport::Multipart,
        ),
    ] {
        let harness = harness(16 * 1024, 1, SUBSCRIPTION);
        let deadlines = TrackingDeadlines::default();
        let headers = json_headers(accept);
        let body = format!(r#"{{"query":"{SUBSCRIPTION}"}}"#).into_bytes();
        let (outcome, body_polls) = invoke(
            &harness.endpoint,
            &deadlines,
            RequestInput {
                method: "POST",
                uri_query: None,
                headers: &headers,
                body,
            },
        );
        let pending = match outcome {
            SubscriptionHttpIngress::Pending(pending) => pending,
            SubscriptionHttpIngress::Finite(response) => {
                panic!("expected pending subscription, got {}", response.status)
            }
        };
        assert_eq!(pending.transport(), expected);
        assert_eq!(body_polls.load(Ordering::SeqCst), 2);
        assert_eq!(harness.signals.admission.load(Ordering::SeqCst), 1);
        assert_eq!(harness.signals.source.load(Ordering::SeqCst), 1);
        assert_eq!(harness.signals.decoded.load(Ordering::SeqCst), 1);
        assert_eq!(harness.signals.deadline_visible.load(Ordering::SeqCst), 1);
        assert_eq!(harness.signals.context.load(Ordering::SeqCst), 0);
        assert_eq!(harness.signals.scalar.load(Ordering::SeqCst), 0);
        assert_eq!(harness.signals.resolver.load(Ordering::SeqCst), 0);
        assert_eq!(harness.endpoint.in_flight_requests(), 1);
        drop(pending);
        assert_eq!(harness.endpoint.in_flight_requests(), 0);
    }
}

#[test]
fn finite_query_mutation_and_get_keep_the_ordinary_path() {
    for source in [
        "query Q($value: Probe = 1) { echo(value: $value) }",
        "mutation M { missing }",
    ] {
        let harness = harness(16 * 1024, 1, SUBSCRIPTION);
        let deadlines = TrackingDeadlines::default();
        let headers = json_headers(b"application/graphql-response+json");
        let body = format!(r#"{{"query":"{source}"}}"#).into_bytes();
        let (outcome, _) = invoke(
            &harness.endpoint,
            &deadlines,
            RequestInput {
                method: "POST",
                uri_query: None,
                headers: &headers,
                body,
            },
        );
        assert!(matches!(outcome, SubscriptionHttpIngress::Finite(_)));
        assert_eq!(harness.endpoint.in_flight_requests(), 0);
        assert_eq!(harness.signals.context.load(Ordering::SeqCst), 1);
    }

    let harness = harness(16 * 1024, 1, SUBSCRIPTION);
    let deadlines = TrackingDeadlines::default();
    let headers = [
        RequestHeader::new("accept", b"application/graphql-response+json"),
        RequestHeader::new("x-chigiri-preflight", b"1"),
    ];
    let (outcome, body_polls) = invoke(
        &harness.endpoint,
        &deadlines,
        RequestInput {
            method: "GET",
            uri_query: Some("query=query+Q%7Becho%7D&operationName=Q"),
            headers: &headers,
            body: b"ignored".to_vec(),
        },
    );
    let SubscriptionHttpIngress::Finite(response) = outcome else {
        panic!("GET query entered subscription path");
    };
    assert_eq!(response.status, 200);
    assert_eq!(body_polls.load(Ordering::SeqCst), 0);
}

#[test]
fn stream_only_accept_never_upgrades_a_query_or_mutation() {
    for source in ["query Q { echo }", "mutation M { missing }"] {
        let harness = harness(16 * 1024, 1, SUBSCRIPTION);
        let deadlines = TrackingDeadlines::default();
        let headers = json_headers(b"text/event-stream");
        let body = format!(r#"{{"query":"{source}"}}"#).into_bytes();
        let (outcome, _) = invoke(
            &harness.endpoint,
            &deadlines,
            RequestInput {
                method: "POST",
                uri_query: None,
                headers: &headers,
                body,
            },
        );
        let SubscriptionHttpIngress::Finite(response) = outcome else {
            panic!("finite operation was upgraded to a stream");
        };
        assert_eq!(response.status, 406);
        assert_eq!(harness.signals.context.load(Ordering::SeqCst), 0);
        assert_eq!(harness.signals.scalar.load(Ordering::SeqCst), 0);
        assert_eq!(harness.signals.resolver.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn finite_only_accept_never_admits_a_selected_subscription() {
    let harness = harness(16 * 1024, 1, SUBSCRIPTION);
    let deadlines = TrackingDeadlines::default();
    let headers = json_headers(b"application/graphql-response+json");
    let body = format!(r#"{{"query":"{SUBSCRIPTION}"}}"#).into_bytes();
    let (outcome, _) = invoke(
        &harness.endpoint,
        &deadlines,
        RequestInput {
            method: "POST",
            uri_query: None,
            headers: &headers,
            body,
        },
    );
    let SubscriptionHttpIngress::Finite(response) = outcome else {
        panic!("finite-only Accept admitted a subscription stream");
    };
    assert_eq!(response.status, 406);
    assert_eq!(response.headers.len(), 1);
    assert_eq!(response.headers[0].name, "content-type");
    assert_eq!(
        response.headers[0].value,
        "application/graphql-response+json; charset=utf-8"
    );
    assert_eq!(
        response.body,
        br#"{"errors":[{"message":"No supported response media type is acceptable","extensions":{"code":"NOT_ACCEPTABLE"}}]}"#
    );
    assert_eq!(harness.endpoint.in_flight_requests(), 0);
    assert_eq!(deadlines.created(SETUP), 0);
    assert_eq!(harness.signals.admission.load(Ordering::SeqCst), 1);
    assert_eq!(harness.signals.source.load(Ordering::SeqCst), 1);
    assert_eq!(harness.signals.decoded.load(Ordering::SeqCst), 1);
    assert_eq!(harness.signals.context.load(Ordering::SeqCst), 0);
    assert_eq!(harness.signals.scalar.load(Ordering::SeqCst), 0);
    assert_eq!(harness.signals.resolver.load(Ordering::SeqCst), 0);
    assert_eq!(
        harness.signals.count_code(
            LifecyclePhase::RequestCompleted,
            LifecycleResult::Rejected,
            None,
        ),
        1
    );
    assert_eq!(
        harness
            .signals
            .count(LifecyclePhase::RequestCompleted, LifecycleResult::Cancelled,),
        0
    );

    let stream_deadlines = TrackingDeadlines::default();
    let stream_headers = json_headers(b"text/event-stream");
    let stream_body = format!(r#"{{"query":"{SUBSCRIPTION}"}}"#).into_bytes();
    let (stream, _) = invoke(
        &harness.endpoint,
        &stream_deadlines,
        RequestInput {
            method: "POST",
            uri_query: None,
            headers: &stream_headers,
            body: stream_body,
        },
    );
    let SubscriptionHttpIngress::Pending(stream) = stream else {
        panic!("finite-media rejection leaked pending capacity");
    };
    assert_eq!(harness.signals.admission.load(Ordering::SeqCst), 2);
    assert_eq!(harness.signals.source.load(Ordering::SeqCst), 2);
    assert_eq!(harness.signals.decoded.load(Ordering::SeqCst), 2);
    assert_eq!(harness.signals.context.load(Ordering::SeqCst), 0);
    drop(stream);
}

#[test]
fn selected_get_subscription_preserves_pinned_405_and_406_precedence() {
    for accept in [
        b"text/event-stream".as_slice(),
        b"multipart/mixed;subscriptionSpec=1.0",
    ] {
        let harness = harness(16 * 1024, 1, SUBSCRIPTION);
        let deadlines = TrackingDeadlines::default();
        let headers = [
            RequestHeader::new("accept", accept),
            RequestHeader::new("x-chigiri-preflight", b"1"),
        ];
        let uri = "query=subscription+S%7Bevent%7D&operationName=S";
        let (outcome, _) = invoke(
            &harness.endpoint,
            &deadlines,
            RequestInput {
                method: "GET",
                uri_query: Some(uri),
                headers: &headers,
                body: Vec::new(),
            },
        );
        let SubscriptionHttpIngress::Finite(response) = outcome else {
            panic!("GET entered the subscription stream");
        };
        assert_eq!(response.status, 405);
        assert!(
            response
                .headers
                .iter()
                .any(|header| header.name == "allow" && header.value == "POST")
        );
    }

    for accept in [
        None,
        Some(b"application/json".as_slice()),
        Some(b"text/event-stream;q=bogus"),
        Some(b"text/event-stream;q=0"),
    ] {
        let harness = harness(16 * 1024, 1, SUBSCRIPTION);
        let deadlines = TrackingDeadlines::default();
        let mut headers = vec![RequestHeader::new("x-chigiri-preflight", b"1")];
        if let Some(accept) = accept {
            headers.push(RequestHeader::new("accept", accept));
        }
        let (outcome, _) = invoke(
            &harness.endpoint,
            &deadlines,
            RequestInput {
                method: "GET",
                uri_query: Some("query=subscription+S%7Bevent%7D&operationName=S"),
                headers: &headers,
                body: Vec::new(),
            },
        );
        let SubscriptionHttpIngress::Finite(response) = outcome else {
            panic!("GET entered the subscription stream");
        };
        assert_eq!(response.status, 406);
        assert!(!response.headers.iter().any(|header| header.name == "allow"));
    }
}

#[test]
fn persisted_source_is_resolved_and_hooked_before_kind_dispatch() {
    let harness = harness(16 * 1024, 1, SUBSCRIPTION);
    let deadlines = TrackingDeadlines::default();
    let headers = json_headers(b"text/event-stream");
    let body = br#"{"operationName":"S","extensions":{"persistedQuery":{"version":1,"sha256Hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}}"#.to_vec();
    let (outcome, _) = invoke(
        &harness.endpoint,
        &deadlines,
        RequestInput {
            method: "POST",
            uri_query: None,
            headers: &headers,
            body,
        },
    );
    assert!(matches!(outcome, SubscriptionHttpIngress::Pending(_)));
    assert_eq!(harness.signals.source.load(Ordering::SeqCst), 1);
    assert_eq!(harness.signals.decoded.load(Ordering::SeqCst), 1);
    assert_eq!(
        harness.signals.decoded_queries.lock().unwrap().as_slice(),
        [SUBSCRIPTION]
    );
    assert_eq!(harness.signals.context.load(Ordering::SeqCst), 0);
}
