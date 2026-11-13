use std::sync::atomic::Ordering;
use std::task::{Context, Poll, Waker};

use chigiri_http::ApplicationMetadata;

use super::super::{WebSocketSeedError, WebSocketUpgradeIngress};
use super::support::{
    AdmissionMode, ManualDeadlines, UPGRADE_TIMEOUT, endpoint, finite, invoke, valid_headers,
};

fn admitted<'deadline>(
    endpoint: &crate::Endpoint<()>,
    deadlines: &'deadline ManualDeadlines,
) -> super::super::AdmittedWebSocketUpgrade<'deadline> {
    match invoke(
        endpoint,
        deadlines,
        "GET",
        &valid_headers(true),
        &ApplicationMetadata::default(),
    ) {
        WebSocketUpgradeIngress::Admitted(seed) => seed,
        WebSocketUpgradeIngress::Finite(response) => {
            panic!(
                "expected admitted seed, received status {}",
                response.status
            )
        }
    }
}

#[test]
fn clone_claims_the_seed_without_resetting_the_original_deadline() {
    let (endpoint, signals) = endpoint(AdmissionMode::Continue, 1, 16 * 1024);
    let clone = endpoint.clone();
    let deadlines = ManualDeadlines::default();
    let seed = admitted(&endpoint, &deadlines);
    let requested_before_claim = deadlines.requested();
    assert_eq!(
        requested_before_claim
            .iter()
            .filter(|duration| **duration == UPGRADE_TIMEOUT)
            .count(),
        1
    );
    let mut claimed = clone.__claim_websocket_upgrade(seed).unwrap();
    assert_eq!(deadlines.requested(), requested_before_claim);
    assert_eq!(claimed.protocol(), "graphql-transport-ws");
    assert_eq!(claimed.key(), "dGhlIHNhbXBsZSBub25jZQ==");
    assert!(claimed.charged_bytes() > 0);
    assert!(claimed.request_telemetry().request_id().is_some());
    claimed.inspect_transport(|transport| {
        assert_eq!(transport.method(), "GET");
        assert_eq!(transport.uri().path(), "/graphql");
        assert!(transport.request_telemetry().is_some());
    });
    assert!(matches!(
        claimed.poll_upgrade_deadline(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
    deadlines.expire(UPGRADE_TIMEOUT);
    assert!(matches!(
        claimed.poll_upgrade_deadline(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(())
    ));
    let debug = format!("{claimed:?}");
    assert!(!debug.contains("dGhlIHNhbXBsZSBub25jZQ=="));
    assert_eq!(signals.admission.load(Ordering::SeqCst), 1);

    let capacity_deadlines = ManualDeadlines::default();
    let capacity = finite(invoke(
        &endpoint,
        &capacity_deadlines,
        "GET",
        &valid_headers(true),
        &ApplicationMetadata::default(),
    ));
    assert_eq!(capacity.status, 503);
    claimed.cancel();
    assert!(matches!(
        invoke(
            &endpoint,
            &ManualDeadlines::default(),
            "GET",
            &valid_headers(true),
            &ApplicationMetadata::default(),
        ),
        WebSocketUpgradeIngress::Admitted(_)
    ));
}

#[test]
fn foreign_claim_consumes_seed_and_releases_only_the_issuing_endpoint() {
    let (issuer, _) = endpoint(AdmissionMode::Continue, 1, 16 * 1024);
    let (foreign, _) = endpoint(AdmissionMode::Continue, 1, 16 * 1024);
    let deadlines = ManualDeadlines::default();
    let seed = admitted(&issuer, &deadlines);
    assert_eq!(
        foreign.__claim_websocket_upgrade(seed).unwrap_err(),
        WebSocketSeedError::ForeignEndpoint
    );

    let issuer_deadlines = ManualDeadlines::default();
    let foreign_deadlines = ManualDeadlines::default();
    let issuer_seed = admitted(&issuer, &issuer_deadlines);
    let foreign_seed = admitted(&foreign, &foreign_deadlines);
    issuer_seed.cancel();
    foreign_seed.cancel();
}

#[test]
fn dropped_unclaimed_seed_releases_the_last_connection_owner() {
    let (endpoint, _) = endpoint(AdmissionMode::Continue, 1, 16 * 1024);
    let deadlines = ManualDeadlines::default();
    let first = admitted(&endpoint, &deadlines);
    let capacity = finite(invoke(
        &endpoint,
        &deadlines,
        "GET",
        &valid_headers(true),
        &ApplicationMetadata::default(),
    ));
    assert_eq!(capacity.status, 503);
    first.cancel();
    admitted(&endpoint, &ManualDeadlines::default()).cancel();
}
