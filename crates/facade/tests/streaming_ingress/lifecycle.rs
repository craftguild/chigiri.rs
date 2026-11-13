use std::sync::atomic::Ordering;
use std::time::Duration;

use chigiri::__streaming::SubscriptionHttpIngress;
use chigiri::{LifecyclePhase, LifecycleResult};

use super::support::{
    OVERALL, RequestInput, SETUP, TrackingDeadlines, harness, invoke, json_headers,
};

const SUBSCRIPTION: &str = "subscription S { event }";

#[test]
fn pending_owns_original_deadlines_and_cancelled_request_observation() {
    let harness = harness(16 * 1024, 1, SUBSCRIPTION);
    let deadlines = TrackingDeadlines::default();
    let headers = json_headers(b"text/event-stream");
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
    let SubscriptionHttpIngress::Pending(pending) = outcome else {
        panic!("selected subscription did not retain pending ownership");
    };

    assert_eq!(body_polls.load(Ordering::SeqCst), 2);
    assert_eq!(deadlines.created(OVERALL), 1);
    assert_eq!(deadlines.created(SETUP), 1);
    assert_eq!(deadlines.live(OVERALL), 1);
    assert_eq!(deadlines.live(SETUP), 1);
    for phase_timeout in 1..=4 {
        let timeout = Duration::from_secs(phase_timeout);
        assert_eq!(deadlines.created(timeout), 1);
        assert_eq!(deadlines.live(timeout), 0);
    }
    for unopened_timeout in 5..=7 {
        assert_eq!(deadlines.created(Duration::from_secs(unopened_timeout)), 0);
    }
    for phase in [
        LifecyclePhase::HttpPreflight,
        LifecyclePhase::TransportAdmission,
        LifecyclePhase::BodyRead,
        LifecyclePhase::HttpDecode,
        LifecyclePhase::OperationSourceResolution,
        LifecyclePhase::DecodedRequestHook,
    ] {
        assert_eq!(harness.signals.count(phase, LifecycleResult::Succeeded), 1);
    }
    assert_eq!(
        harness
            .signals
            .count(LifecyclePhase::RequestStarted, LifecycleResult::Started),
        1
    );
    assert_eq!(
        harness
            .signals
            .count(LifecyclePhase::RequestCompleted, LifecycleResult::Cancelled,),
        0
    );
    assert_eq!(harness.endpoint.in_flight_requests(), 1);

    drop(pending);

    assert_eq!(deadlines.live(OVERALL), 0);
    assert_eq!(deadlines.live(SETUP), 0);
    assert_eq!(harness.endpoint.in_flight_requests(), 0);
    assert_eq!(
        harness
            .signals
            .count(LifecyclePhase::RequestCompleted, LifecycleResult::Cancelled,),
        1
    );
    assert_eq!(
        harness
            .signals
            .count(LifecyclePhase::RequestCompleted, LifecycleResult::Rejected,),
        0
    );
    assert_eq!(harness.signals.context.load(Ordering::SeqCst), 0);
    assert_eq!(harness.signals.scalar.load(Ordering::SeqCst), 0);
    assert_eq!(harness.signals.resolver.load(Ordering::SeqCst), 0);
}

#[test]
fn finite_pre_worker_rejection_completes_instead_of_cancelling() {
    let harness = harness(16 * 1024, 1, SUBSCRIPTION);
    let deadlines = TrackingDeadlines::default();
    let headers = json_headers(b"text/event-stream");
    let body = br#"{"query":"query Q { echo }"}"#.to_vec();
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
        panic!("query was incorrectly upgraded to a stream");
    };

    assert_eq!(response.status, 406);
    assert_eq!(harness.endpoint.in_flight_requests(), 0);
    assert_eq!(
        harness
            .signals
            .count(LifecyclePhase::RequestCompleted, LifecycleResult::Rejected,),
        1
    );
    assert_eq!(
        harness
            .signals
            .count(LifecyclePhase::RequestCompleted, LifecycleResult::Cancelled,),
        0
    );
    assert_eq!(deadlines.live(OVERALL), 0);
    assert_eq!(deadlines.created(SETUP), 0);
}
