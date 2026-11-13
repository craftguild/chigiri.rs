use super::test_support::{graph_request, prepared, subscriber};
use super::*;
use chigiri_http::MetadataField;
use chigiri_runtime::Value;

#[cfg(feature = "subscription-worker")]
#[path = "tests/finite.rs"]
mod finite;
#[cfg(feature = "subscription-worker")]
#[path = "tests/late_host.rs"]
mod late_host;

#[test]
fn accepted_metadata_limit_boundaries_are_fallible_and_exact() {
    assert_eq!(
        MetadataRetentionLimit::new(0),
        Err(RetentionError::InvalidLimit)
    );
    assert!(MetadataRetentionLimit::new(64 * 1024).is_ok());
    assert_eq!(
        MetadataRetentionLimit::new(64 * 1024 + 1),
        Err(RetentionError::InvalidLimit)
    );
    let limit = MetadataRetentionLimit::default();
    assert!(limit.check(16 * 1024).is_ok());
    assert_eq!(limit.check(16 * 1024 + 1), Err(RetentionError::Exceeded));
}

#[test]
fn transport_charge_counts_owned_bytes_and_collection_entries() {
    let metadata =
        ApplicationMetadata::new(vec![MetadataField::new("app", b"xyz").unwrap()]).unwrap();
    let headers = [RequestHeader::new("authorization", b"secret")];
    let trace = TraceId::new("trace-id").unwrap();
    let transport =
        TransportRequest::new("POST", "/graphql", Some("q=1"), &headers, None, &metadata)
            .with_trace_id(&trace);
    let mut charge = SubscriptionMetadataCharge::new();
    charge_transport(&mut charge, transport).unwrap();
    assert_eq!(charge.bytes(), 4 + 8 + 3 + 16 + 13 + 6 + 16 + 3 + 3 + 8);
}

#[test]
fn retention_charges_before_copy_and_accepts_equality_only() {
    let metadata = ApplicationMetadata::default();
    let headers = [RequestHeader::new("authorization", b"secret")];
    let transport = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    let prepared = prepared(graph_request());
    let expected = prepared.retained_metadata_bytes().unwrap() + 4 + 8 + 16 + 13 + 6;
    let retain = |limit| {
        SubscriberRequest::retain(
            transport,
            &prepared,
            RequestMethod::Post,
            ResponseMediaType::Json,
            SubscriptionTransport::ServerSentEvents,
            None,
            MetadataRetentionLimit::new(limit).unwrap(),
        )
    };
    let retained = retain(expected).unwrap();
    assert_eq!(retained.charged_bytes(), expected);
    assert_eq!(retain(expected - 1).unwrap_err(), RetentionError::Exceeded);
    assert!(Arc::ptr_eq(
        retained.request.as_ref().unwrap(),
        &prepared.retained_request()
    ));
    assert!(Arc::ptr_eq(
        &retained.identity,
        &prepared.retained_identity()
    ));
}

#[test]
fn authorized_initialization_is_charged_only_for_websocket() {
    let metadata = ApplicationMetadata::default();
    let transport = TransportRequest::new("GET", "/graphql", None, &[], None, &metadata);
    let prepared = prepared(graph_request());
    let init: Object = [("token", Value::from("init-secret"))]
        .into_iter()
        .collect();
    let retain = |profile, init| {
        SubscriberRequest::retain(
            transport,
            &prepared,
            RequestMethod::Get,
            ResponseMediaType::Json,
            profile,
            init,
            MetadataRetentionLimit::default(),
        )
    };
    let without = retain(SubscriptionTransport::WebSocket, None).unwrap();
    let with = retain(SubscriptionTransport::WebSocket, Some(init.clone())).unwrap();
    assert_eq!(
        with.charged_bytes() - without.charged_bytes(),
        32 + 2 * 5 + 11
    );
    assert_eq!(
        retain(SubscriptionTransport::Multipart, Some(init)).unwrap_err(),
        RetentionError::InitializationForHttp
    );
}

#[test]
fn retained_metadata_diagnostics_do_not_disclose_credentials_or_query() {
    let request = subscriber(SubscriptionTransport::WebSocket);
    let debug = format!("{request:?}");
    for secret in [
        "original-secret",
        "private=query",
        "subscription Named",
        "/graphql",
    ] {
        assert!(!debug.contains(secret));
    }
    assert!(debug.contains("charged_bytes"));
}

#[test]
#[cfg(feature = "subscription-worker")]
fn websocket_ingress_shares_init_allocations_but_charges_each_operation_fully() {
    let metadata = ApplicationMetadata::default();
    let transport = TransportRequest::new("GET", "/graphql", None, &[], None, &metadata);
    let request = graph_request();
    let prepared = test_support::schema()
        .prepare_websocket_operation(request.clone())
        .unwrap();
    let init = Arc::new(
        [("token", Value::from("init-secret"))]
            .into_iter()
            .collect::<Object>(),
    );
    let capture = || {
        SubscriberIngress::capture_websocket(
            transport,
            &request,
            RequestMethod::Post,
            ResponseMediaType::GraphQlResponseJson,
            Some(Arc::clone(&init)),
            MetadataRetentionLimit::default(),
        )
        .unwrap()
    };

    let first =
        SubscriberRequest::from_websocket(capture(), &prepared, MetadataRetentionLimit::default())
            .unwrap();
    let second =
        SubscriberRequest::from_websocket(capture(), &prepared, MetadataRetentionLimit::default())
            .unwrap();

    assert!(Arc::ptr_eq(
        first.ingress.authorized_init.as_ref().unwrap(),
        &init
    ));
    assert!(Arc::ptr_eq(
        second.ingress.authorized_init.as_ref().unwrap(),
        &init
    ));
    assert_eq!(first.charged_bytes(), second.charged_bytes());
    let without = SubscriberRequest::from_websocket(
        SubscriberIngress::capture_websocket(
            transport,
            &request,
            RequestMethod::Post,
            ResponseMediaType::GraphQlResponseJson,
            None,
            MetadataRetentionLimit::default(),
        )
        .unwrap(),
        &prepared,
        MetadataRetentionLimit::default(),
    )
    .unwrap();
    assert_eq!(
        first.charged_bytes() - without.charged_bytes(),
        32 + 2 * 5 + 11
    );
}
