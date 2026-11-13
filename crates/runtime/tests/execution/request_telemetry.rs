//! Test observer contracts when request-correlation generation fails.

use std::sync::{Arc, Mutex};

use chigiri_runtime::{
    ErrorEvent, ErrorObserver, ExecutableSchema, GraphQlRequest, LifecyclePhase, RequestId,
    RequestIdFactory, RequestIdFactoryError,
};

use super::{Dispatch, SCHEMA, block_on};

type FailureRecord = (
    bool,
    bool,
    LifecyclePhase,
    &'static str,
    &'static str,
    String,
);

#[derive(Clone, Default)]
struct FailureRecorder(Arc<Mutex<Vec<FailureRecord>>>);

impl ErrorObserver for FailureRecorder {
    fn observe_error(&self, event: ErrorEvent<'_>) {
        self.0.lock().unwrap().push((
            event.error_id().is_some(),
            event.identity().request_id().is_some(),
            event.phase(),
            event.code(),
            event.component(),
            event.source().to_string(),
        ));
    }
}

struct FailingRequestIds;

impl RequestIdFactory for FailingRequestIds {
    fn create(&self) -> Result<RequestId, RequestIdFactoryError> {
        Err(RequestIdFactoryError::new(FactoryFailure))
    }
}

#[derive(Debug)]
struct FactoryFailure;

impl std::fmt::Display for FactoryFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("random source unavailable")
    }
}

impl std::error::Error for FactoryFailure {}

#[test]
fn obs_02_notifies_the_observer_when_request_id_creation_fails() {
    let recorder = FailureRecorder::default();
    let schema = ExecutableSchema::builder(&SCHEMA, Dispatch)
        .request_id_factory(FailingRequestIds)
        .error_observer(recorder.clone())
        .build()
        .unwrap();

    let response = block_on(schema.execute(GraphQlRequest::new("{ hello }"), ()));

    assert!(response.errors.is_empty());
    assert!(response.data.is_some());
    let records = recorder.0.lock().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0],
        (
            false,
            false,
            LifecyclePhase::RequestStarted,
            "REQUEST_ID_GENERATION_FAILED",
            "request-id-factory",
            "request ID generation failed".to_owned(),
        )
    );
}
