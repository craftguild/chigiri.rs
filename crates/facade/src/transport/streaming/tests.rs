//! Closed pre-worker rejection mapping and initial-request observation proof.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chigiri_http::ResponseMediaType;
use chigiri_runtime::{
    ErrorEvent, ErrorObserver, LifecycleEvent, LifecycleObserver, LifecyclePhase, LifecycleResult,
};

use super::{InitialRequestObservation, SubscriptionHttpIngress, admission_failure};
use crate::subscription::bridge_resources::BridgeResourceError;
use crate::subscription::request::test_support::schema;
use crate::transport::InFlightRequest;

type Event = (LifecyclePhase, LifecycleResult, Option<&'static str>);

#[derive(Clone, Default)]
struct Observations {
    events: Arc<Mutex<Vec<Event>>>,
    internal: Arc<AtomicUsize>,
}

impl LifecycleObserver for Observations {
    fn observe(&self, event: LifecycleEvent<'_>) {
        self.events
            .lock()
            .unwrap()
            .push((event.phase(), event.result(), event.code()));
    }
}

impl ErrorObserver for Observations {
    fn observe_error(&self, _: ErrorEvent<'_>) {
        self.internal.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn pre_worker_capacity_is_a_completed_safe_overload_not_abandonment() {
    for media in [
        ResponseMediaType::GraphQlResponseJson,
        ResponseMediaType::Json,
    ] {
        let observations = Observations::default();
        let schema = schema()
            .reconfigure()
            .lifecycle_observer(observations.clone())
            .build();
        let count = Arc::new(AtomicUsize::new(0));
        let observation = InitialRequestObservation::new(
            schema.clone(),
            schema.request_telemetry(None),
            InFlightRequest::start(Arc::clone(&count)),
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let SubscriptionHttpIngress::Finite(response) =
            admission_failure(&schema, media, BridgeResourceError::Capacity, observation)
        else {
            panic!("capacity cannot create a worker input");
        };
        assert_eq!(response.status, 503);
        let body = std::str::from_utf8(&response.body).unwrap();
        assert!(body.contains("Subscription capacity is unavailable"));
        assert!(body.contains("SUBSCRIPTION_LIMIT_EXCEEDED"));
        assert!(response.headers.iter().any(|header| header.name == "content-type" && header.value == media.content_type()));
        assert!(
            !response
                .headers
                .iter()
                .any(|header| matches!(header.name.as_str(), "allow" | "retry-after"))
        );
        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert_eq!(
            *observations.events.lock().unwrap(),
            [
                (
                    LifecyclePhase::ServerBuilt,
                    LifecycleResult::Succeeded,
                    None
                ),
                (
                    LifecyclePhase::RequestStarted,
                    LifecycleResult::Started,
                    None
                ),
                (
                    LifecyclePhase::RequestCompleted,
                    LifecycleResult::Rejected,
                    Some("SUBSCRIPTION_LIMIT_EXCEEDED")
                ),
            ]
        );
    }
}

#[test]
fn every_non_capacity_admission_failure_is_masked_and_observed_once() {
    for error in [
        BridgeResourceError::Invariant,
        BridgeResourceError::GenerationExhausted,
        BridgeResourceError::DuplicateId,
        BridgeResourceError::InvalidId,
        BridgeResourceError::StaleGeneration,
        BridgeResourceError::InvalidOperationKind,
    ] {
        let observations = Observations::default();
        let schema = schema()
            .reconfigure()
            .lifecycle_observer(observations.clone())
            .error_observer(observations.clone())
            .build();
        let count = Arc::new(AtomicUsize::new(0));
        let observation = InitialRequestObservation::new(
            schema.clone(),
            schema.request_telemetry(None),
            InFlightRequest::start(Arc::clone(&count)),
        );
        let SubscriptionHttpIngress::Finite(response) = admission_failure(
            &schema,
            ResponseMediaType::GraphQlResponseJson,
            error,
            observation,
        ) else {
            panic!("an internal quota failure cannot create a worker input");
        };
        assert_eq!(response.status, 500);
        let body = std::str::from_utf8(&response.body).unwrap();
        assert!(body.contains("Internal server error"));
        assert!(body.contains("INTERNAL_SERVER_ERROR"));
        assert!(!body.contains("subscription resource admission failed"));
        assert!(!body.contains(&format!("{error:?}")));
        assert_eq!(observations.internal.load(Ordering::SeqCst), 1);
        assert_eq!(count.load(Ordering::SeqCst), 0);
        let events = observations.events.lock().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.0 == LifecyclePhase::RequestCompleted)
                .count(),
            1
        );
        assert_eq!(
            events.last().unwrap(),
            &(
                LifecyclePhase::RequestCompleted,
                LifecycleResult::Failed,
                Some("INTERNAL_SERVER_ERROR")
            )
        );
        assert!(
            !events
                .iter()
                .any(|event| event.1 == LifecycleResult::Cancelled)
        );
    }
}

#[test]
fn true_initial_request_abandonment_records_only_one_cancelled_completion() {
    let observations = Observations::default();
    let schema = schema()
        .reconfigure()
        .lifecycle_observer(observations.clone())
        .build();
    let count = Arc::new(AtomicUsize::new(0));
    let observation = InitialRequestObservation::new(
        schema.clone(),
        schema.request_telemetry(None),
        InFlightRequest::start(Arc::clone(&count)),
    );
    drop(observation);
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert_eq!(
        *observations.events.lock().unwrap(),
        [
            (
                LifecyclePhase::ServerBuilt,
                LifecycleResult::Succeeded,
                None
            ),
            (
                LifecyclePhase::RequestStarted,
                LifecycleResult::Started,
                None
            ),
            (
                LifecyclePhase::RequestCompleted,
                LifecycleResult::Cancelled,
                None
            ),
        ]
    );
}
