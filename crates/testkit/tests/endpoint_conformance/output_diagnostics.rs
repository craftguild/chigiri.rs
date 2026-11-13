//! Verifies diagnostic correlation for completion limits at HTTP endpoints.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chigiri::{
    ErrorEvent, ErrorId, ErrorIdFactory, ErrorIdFactoryError, ErrorObserver, ExecutionLimits,
    PathSegment,
};

use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Record {
    error_id: Option<String>,
    source: String,
    path: Vec<PathSegment>,
    component: &'static str,
    output_limit: bool,
}

#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<Record>>>);

impl ErrorObserver for Recorder {
    fn observe_error(&self, event: ErrorEvent<'_>) {
        self.0.lock().unwrap().push(Record {
            error_id: event.error_id().map(|id| id.as_str().to_owned()),
            source: event.source().to_string(),
            path: event.path().unwrap_or_default().to_vec(),
            component: event.component(),
            output_limit: event
                .source()
                .is::<chigiri::core::executor::OutputLimitExceeded>(),
        });
    }
}

struct SequentialIds(AtomicU64);

impl ErrorIdFactory for SequentialIds {
    fn create(&self) -> Result<ErrorId, ErrorIdFactoryError> {
        let value = self.0.fetch_add(1, Ordering::SeqCst);
        ErrorId::new(format!("http-output-{value}")).map_err(ErrorIdFactoryError::new)
    }
}

#[tokio::test]
async fn http_field_list_errors_are_correlated_with_masked_ids_and_sources() {
    let mut limits = ExecutionLimits::production();
    limits.max_list_items_per_field = 1;
    let (endpoint, recorder) = observed_endpoint(limits);
    let response = post(endpoint, r#"{"query":"{ items }"}"#).await;

    assert_correlated(&response, &recorder, &[PathSegment::from("items")]);
}

#[tokio::test]
async fn http_request_output_limit_errors_are_correlated_with_masked_ids_and_sources() {
    let mut limits = ExecutionLimits::production();
    limits.max_output_nodes = 1;
    let (endpoint, recorder) = observed_endpoint(limits);
    let response = post(endpoint, r#"{"query":"{ item { name } }"}"#).await;

    assert_correlated(
        &response,
        &recorder,
        &[PathSegment::from("item"), PathSegment::from("name")],
    );
}

fn observed_endpoint(limits: ExecutionLimits) -> (Endpoint<TestContext>, Recorder) {
    let recorder = Recorder::default();
    let executable = lifecycle_support::schema_builder(
        PhaseLog::default(),
        Arc::new(AtomicUsize::new(0)),
        false,
    )
    .build()
    .unwrap();
    let endpoint = Endpoint::builder(executable)
        .inject(|components| {
            components.execution().limits(limits);
            components
                .observability()
                .error_id_factory(SequentialIds(AtomicU64::new(1)))
                .error_observer(recorder.clone());
        })
        .context_factory(Factory(FactoryMode::Ready))
        .build()
        .unwrap();
    (endpoint, recorder)
}

fn assert_correlated(
    response: &chigiri_http::ResponseParts,
    recorder: &Recorder,
    expected_path: &[PathSegment],
) {
    assert_eq!(response.status, 200);
    let body = json(response);
    let response_id = body["errors"][0]["extensions"]["errorId"].as_str().unwrap();
    assert_eq!(body["errors"][0]["message"], "Internal server error");
    assert!(!String::from_utf8_lossy(&response.body).contains("response output limit exceeded"));

    let records = recorder.0.lock().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].error_id.as_deref(), Some(response_id));
    assert_eq!(records[0].source, "response output limit exceeded");
    assert_eq!(records[0].path, expected_path);
    assert_eq!(records[0].component, "completion");
    assert!(records[0].output_limit);
}
