use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use chigiri_http::{ApplicationMetadata, MetadataField, RequestHeader, TransportRequest};
use chigiri_observability::{MetricLabel, MetricLabelKey, MetricLabels, MetricOperationKind};
use chigiri_runtime::{
    ErrorEvent, ErrorId, GraphQlRequest, LifecyclePhase, Object, PathSegment, RequestId,
    RequestIdentity, SchemaDigest, TraceId,
};

#[test]
fn obs_05_default_debug_redacts_queries_variables_headers_ip_pii_and_sources() {
    let mut variables = Object::new();
    variables.insert("email", "alice@example.test");
    let request =
        GraphQlRequest::new("query Secret { customerPassword }").with_variables(variables);
    let request_debug = format!("{request:?}");
    assert!(!request_debug.contains("customerPassword"));
    assert!(!request_debug.contains("alice@example.test"));

    let headers = [RequestHeader::new(
        "authorization",
        b"Bearer customer-token",
    )];
    let metadata = ApplicationMetadata::new(vec![
        MetadataField::new("principal", b"alice@example.test".to_vec()).unwrap(),
    ])
    .unwrap();
    let transport = TransportRequest::new(
        "POST",
        "/graphql",
        Some("query=%7BcustomerPassword%7D"),
        &headers,
        Some(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(203, 0, 113, 4)),
            443,
        )),
        &metadata,
    );
    let transport_debug = format!("{transport:?}");
    for secret in [
        "customerPassword",
        "customer-token",
        "alice@example.test",
        "203.0.113.4",
    ] {
        assert!(!transport_debug.contains(secret));
    }

    let source = SecretSource;
    let error_id = ErrorId::new("private-error-id").unwrap();
    let request_id = RequestId::new("private-request-id").unwrap();
    let trace_id = TraceId::new("private-trace-id").unwrap();
    let path = [PathSegment::from("customerPassword")];
    let event = ErrorEvent::new(
        Some(&error_id),
        RequestIdentity::new(Some(&request_id), Some(&trace_id), None),
        LifecyclePhase::Execution,
        "INTERNAL_SERVER_ERROR",
        &source,
        Some(&path),
        Some("Customer.password"),
        "resolver",
    );
    let debug = format!("{event:?}");
    for secret in [
        "private-error-id",
        "private-request-id",
        "private-trace-id",
        "customerPassword",
        "Customer.password",
        "database-password",
    ] {
        assert!(!debug.contains(secret));
    }
}

#[test]
fn obs_06_metric_labels_use_closed_keys_and_enforce_the_64_byte_limit() {
    let labels = MetricLabels::new(vec![
        MetricLabel::operation_kind(MetricOperationKind::Query),
        MetricLabel::registered_operation_key("catalog-key-1").unwrap(),
        MetricLabel::schema_digest(SchemaDigest::from_bytes([7; 32])),
        MetricLabel::lifecycle_phase(LifecyclePhase::Execution),
        MetricLabel::result_code("SUCCEEDED").unwrap(),
        MetricLabel::adapter("axum").unwrap(),
        MetricLabel::provider("token-bucket").unwrap(),
    ])
    .unwrap();
    assert_eq!(labels.as_slice().len(), 7);
    assert_eq!(labels.as_slice()[0].key(), MetricLabelKey::OperationKind);
    assert!(
        labels
            .as_slice()
            .iter()
            .all(|label| label.value().len() <= 64)
    );

    assert!(MetricLabel::registered_operation_key("{ customerPassword }").is_err());
    assert!(MetricLabel::provider("authorization: Bearer secret").is_err());
    assert!(MetricLabel::result_code("x".repeat(65).leak()).is_err());
    let duplicate = vec![
        MetricLabel::adapter("axum").unwrap(),
        MetricLabel::adapter("hyper").unwrap(),
    ];
    assert!(MetricLabels::new(duplicate).is_err());
}

struct SecretSource;

impl std::fmt::Debug for SecretSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretSource")
    }
}

impl std::fmt::Display for SecretSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("database-password=never-log-this")
    }
}

impl std::error::Error for SecretSource {}
