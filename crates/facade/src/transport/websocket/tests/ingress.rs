use std::sync::atomic::Ordering;

use chigiri_http::{ApplicationMetadata, RequestHeader};
use chigiri_runtime::{LifecyclePhase, LifecycleResult};

use super::super::WebSocketUpgradeIngress;
use super::support::{
    ADMISSION_TIMEOUT, AdmissionMode, ManualDeadlines, assert_no_execution, endpoint, finite,
    invoke, valid_headers,
};

#[test]
fn valid_opening_runs_shared_admission_once_without_execution() {
    let (endpoint, signals) = endpoint(AdmissionMode::Continue, 1, 16 * 1024);
    let deadlines = ManualDeadlines::default();
    let metadata = ApplicationMetadata::default();
    let result = invoke(
        &endpoint,
        &deadlines,
        "GET",
        &valid_headers(true),
        &metadata,
    );
    assert!(matches!(result, WebSocketUpgradeIngress::Admitted(_)));
    assert_eq!(signals.admission.load(Ordering::SeqCst), 1);
    assert_no_execution(&signals);
    assert_eq!(
        *signals.lifecycle.lock().unwrap(),
        [
            (LifecyclePhase::RequestStarted, LifecycleResult::Started),
            (LifecyclePhase::HttpPreflight, LifecycleResult::Started),
            (LifecyclePhase::HttpPreflight, LifecycleResult::Succeeded),
            (LifecyclePhase::TransportAdmission, LifecycleResult::Started,),
            (
                LifecyclePhase::TransportAdmission,
                LifecycleResult::Succeeded,
            ),
            (LifecyclePhase::RequestCompleted, LifecycleResult::Succeeded,),
        ]
    );
}

#[test]
fn handshake_and_production_csrf_reject_before_custom_admission() {
    let (endpoint, signals) = endpoint(AdmissionMode::Continue, 1, 16 * 1024);
    let deadlines = ManualDeadlines::default();
    let metadata = ApplicationMetadata::default();
    let invalid = finite(invoke(
        &endpoint,
        &deadlines,
        "POST",
        &valid_headers(true),
        &metadata,
    ));
    assert_eq!(invalid.status, 405);
    let csrf = finite(invoke(
        &endpoint,
        &deadlines,
        "GET",
        &valid_headers(false),
        &metadata,
    ));
    assert_eq!(csrf.status, 403);
    assert!(String::from_utf8_lossy(&csrf.body).contains("CSRF_PREVENTED"));
    assert_eq!(signals.admission.load(Ordering::SeqCst), 0);
    assert_no_execution(&signals);
}

#[test]
fn application_rejection_and_internal_failure_are_safe_finite_responses() {
    let metadata = ApplicationMetadata::default();
    for (mode, status, visible, secret) in [
        (AdmissionMode::Reject, 429, "NOT_NOW", "admission failed"),
        (
            AdmissionMode::Internal,
            500,
            "INTERNAL_SERVER_ERROR",
            "admission failed",
        ),
    ] {
        let (endpoint, signals) = endpoint(mode, 1, 16 * 1024);
        let response = finite(invoke(
            &endpoint,
            &ManualDeadlines::default(),
            "GET",
            &valid_headers(true),
            &metadata,
        ));
        let body = String::from_utf8_lossy(&response.body);
        assert_eq!(response.status, status);
        assert!(body.contains(visible));
        assert!(!body.contains(secret));
        assert_eq!(signals.admission.load(Ordering::SeqCst), 1);
        assert_no_execution(&signals);
    }
}

#[test]
fn phase_and_original_upgrade_timeouts_are_finite_and_do_not_poll_execution() {
    let metadata = ApplicationMetadata::default();
    for expired in [ADMISSION_TIMEOUT, super::support::UPGRADE_TIMEOUT] {
        let (endpoint, signals) = endpoint(AdmissionMode::Pending, 1, 16 * 1024);
        let deadlines = ManualDeadlines::default();
        deadlines.expire(expired);
        let response = finite(invoke(
            &endpoint,
            &deadlines,
            "GET",
            &valid_headers(true),
            &metadata,
        ));
        assert_eq!(response.status, 504);
        assert!(String::from_utf8_lossy(&response.body).contains("REQUEST_TIMEOUT"));
        assert_eq!(signals.admission.load(Ordering::SeqCst), 0);
        assert_no_execution(&signals);
    }
}

#[test]
fn retained_metadata_is_rejected_before_connection_admission() {
    let (endpoint, signals) = endpoint(AdmissionMode::Continue, 1, 1024);
    let deadlines = ManualDeadlines::default();
    let metadata = ApplicationMetadata::default();
    let private = vec![b'x'; 2 * 1024];
    let mut oversized = valid_headers(true);
    oversized.push(RequestHeader::new("private", &private));
    let response = finite(invoke(&endpoint, &deadlines, "GET", &oversized, &metadata));
    assert_eq!(response.status, 413);
    let body = String::from_utf8_lossy(&response.body);
    assert!(body.contains("BAD_USER_INPUT"));
    assert!(!body.contains(&"x".repeat(32)));
    assert_no_execution(&signals);

    let accepted = invoke(
        &endpoint,
        &deadlines,
        "GET",
        &valid_headers(true),
        &ApplicationMetadata::default(),
    );
    assert!(matches!(accepted, WebSocketUpgradeIngress::Admitted(_)));
}

#[test]
fn handshake_header_collection_is_bounded_with_no_unchecked_escape() {
    let (endpoint, _) = endpoint(AdmissionMode::Continue, 1, 1024);
    let deadlines = ManualDeadlines::default();
    let metadata = ApplicationMetadata::default();
    let mut headers = valid_headers(true);
    headers.push(RequestHeader::new("private", &[b'x'; 900]));
    let response = finite(invoke(&endpoint, &deadlines, "GET", &headers, &metadata));
    assert_eq!(response.status, 413);
}
