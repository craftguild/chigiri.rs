//! Checks benchmark-fixture profile contracts and observer-queue delivery.

use super::fixture::{
    DiagnosticProfile, Fixture, LIFECYCLE_EVENTS_PER_REQUEST, Profile,
    diagnostic_profile_definitions, profile_definitions,
};
use super::measure::{QUERY, TokioDeadline};
use chigiri_http::{ApplicationMetadata, BufferedBody, RequestHeader, TransportRequest};

#[test]
fn profile_definitions_match_the_five_closed_profiles() {
    let definitions = profile_definitions();
    let names = definitions
        .iter()
        .map(|profile| profile.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, Profile::ALL.map(Profile::name));
    let facade = definitions.last().unwrap();
    assert!(
        facade
            .includes
            .iter()
            .any(|value| value == "Endpoint component graph composition")
    );
}

#[test]
fn diagnostic_profile_definitions_match_the_five_closed_profiles() {
    let definitions = diagnostic_profile_definitions();
    assert_eq!(
        definitions
            .iter()
            .map(|profile| profile.name.as_str())
            .collect::<Vec<_>>(),
        DiagnosticProfile::ALL.map(DiagnosticProfile::name)
    );
    assert!(definitions.iter().all(|profile| {
        !profile.includes.is_empty()
            && !profile.excludes.is_empty()
            && profile.name.starts_with("diagnostic-")
    }));
}

#[tokio::test]
async fn every_profile_executes_the_same_query_successfully() {
    let headers = [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("accept", b"application/graphql-response+json"),
    ];
    let body = serde_json::to_vec(&serde_json::json!({"query": QUERY})).unwrap();
    let metadata = ApplicationMetadata::default();
    for profile in Profile::ALL {
        let fixture = Fixture::new(profile, true).unwrap();
        let request = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
        let response = fixture
            .endpoint
            .handle(request, BufferedBody::new(body.clone()), &TokioDeadline)
            .await;
        assert_eq!(response.status, 200, "profile: {}", profile.name());
        let response: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert!(
            response.get("errors").is_none(),
            "profile: {}",
            profile.name()
        );
        assert!(response["data"]["books"].is_array());
    }
}

#[tokio::test]
async fn diagnostic_profiles_use_queues_only_when_configured() {
    for profile in DiagnosticProfile::ALL {
        let fixture = Fixture::new_diagnostic(profile).unwrap();
        let measured = super::measure::endpoint_profile_samples(&[&fixture], 1, 1)
            .await
            .unwrap();
        let expected_queue = matches!(
            profile,
            DiagnosticProfile::QueueOnly | DiagnosticProfile::FullObserver
        );
        assert_eq!(measured.observer_queues.len(), usize::from(expected_queue));
        if expected_queue {
            assert_eq!(
                measured.observer_queues[0].accepted_lifecycle_event_count,
                u64::try_from(LIFECYCLE_EVENTS_PER_REQUEST).unwrap()
            );
        }
    }
}

#[test]
fn request_id_diagnostic_profiles_distinguish_random_and_fixed_telemetry() {
    let random = Fixture::new_diagnostic(DiagnosticProfile::RandomRequestId).unwrap();
    let random_first = random.endpoint.request_telemetry(None);
    let random_second = random.endpoint.request_telemetry(None);
    assert_ne!(
        random_first.request_id().unwrap().as_str(),
        random_second.request_id().unwrap().as_str()
    );

    let fixed = Fixture::new_diagnostic(DiagnosticProfile::TelemetryClone).unwrap();
    for _ in 0..2 {
        assert_eq!(
            fixed
                .endpoint
                .request_telemetry(None)
                .request_id()
                .unwrap()
                .as_str(),
            "benchmark-request-id"
        );
    }
}

#[tokio::test]
async fn observer_profiles_deliver_the_expected_build_and_request_events() {
    for profile in [Profile::Observer, Profile::Facade] {
        let fixture = Fixture::new(profile, true).unwrap();
        let measured = super::measure::endpoint_profile_samples(&[&fixture], 1, 1)
            .await
            .unwrap();
        assert_eq!(measured.observer_queues.len(), 1);
        assert_eq!(
            measured.observer_queues[0].accepted_lifecycle_event_count,
            u64::try_from(LIFECYCLE_EVENTS_PER_REQUEST).unwrap()
        );
        assert_eq!(LIFECYCLE_EVENTS_PER_REQUEST, 29);
    }
}
