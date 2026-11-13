#[path = "axum_harness.rs"]
mod axum_harness;
#[path = "hyper_harness.rs"]
mod hyper_harness;
#[path = "hyper_wire.rs"]
mod hyper_wire;
#[path = "lifecycle_support.rs"]
mod lifecycle_support;
#[path = "neutral_harness.rs"]
mod neutral_harness;
#[path = "observer_axum.rs"]
mod observer_axum;

use chigiri_testkit::assert_lifecycle_conformance;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

#[tokio::test]
async fn life_01_to_08_axum_runs_the_common_endpoint_suite() {
    assert_lifecycle_conformance(&axum_harness::AxumHarness)
        .await
        .unwrap();
}

#[tokio::test]
async fn life_01_to_08_hyper_runs_through_a_common_endpoint_suite() {
    assert_lifecycle_conformance(&hyper_harness::HyperHarness)
        .await
        .unwrap();
}

#[tokio::test]
async fn life_01_to_08_direct_runs_through_a_common_endpoint_suite() {
    assert_lifecycle_conformance(&neutral_harness::NeutralHarness)
        .await
        .unwrap();
}

#[tokio::test]
async fn life_05_direct_execution_starts_with_the_request_plugin_not_the_http_phase() {
    let log = lifecycle_support::PhaseLog::default();
    let response = lifecycle_support::schema(log.clone(), Arc::new(AtomicUsize::new(0)), false)
        .execute(
            chigiri::GraphQlRequest::new("{ hello }"),
            lifecycle_support::TestContext { log: log.clone() },
        )
        .await;
    assert!(response.errors.is_empty());
    assert_eq!(log.snapshot(), ["request", "operation", "resolver"]);
}

#[tokio::test]
async fn obs_04_full_observer_queue_does_not_change_the_response() {
    let log = lifecycle_support::PhaseLog::default();
    let (exporter, _, _) = chigiri_observability::LifecycleExporter::new(1).unwrap();
    let schema =
        lifecycle_support::schema_builder(log.clone(), Arc::new(AtomicUsize::new(0)), false)
            .lifecycle_observer(exporter.clone())
            .build()
            .unwrap();
    let response = schema
        .execute(
            chigiri::GraphQlRequest::new("{ hello }"),
            lifecycle_support::TestContext { log },
        )
        .await;

    assert!(response.errors.is_empty());
    assert_eq!(
        response
            .data
            .unwrap()
            .as_object()
            .unwrap()
            .get("hello")
            .and_then(chigiri::Value::as_str),
        Some("ok")
    );
    assert!(exporter.dropped_lifecycle() > 0);
}
