//! Test error-ID observation when ID generation fails.

use std::sync::{Arc, Mutex};

use chigiri_runtime::__private::{
    ArgumentValues, BoxFuture, ResolvedObject, ResolvedValue, ResolverDispatch,
};
use chigiri_runtime::core::executable::CustomScalarRegistry;
use chigiri_runtime::{
    ErrorEvent, ErrorId, ErrorIdFactory, ErrorIdFactoryError, ErrorObserver, ExecutableSchema,
    FieldError, FieldResult, GraphQlRequest, LifecyclePhase, Value,
};

use super::{SCHEMA, block_on};

type Record = (bool, &'static str, &'static str);

#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<Record>>>);

impl ErrorObserver for Recorder {
    fn observe_error(&self, event: ErrorEvent<'_>) {
        self.0
            .lock()
            .unwrap()
            .push((event.error_id().is_some(), event.code(), event.component()));
    }
}

struct FailingIds;

impl ErrorIdFactory for FailingIds {
    fn create(&self) -> Result<ErrorId, ErrorIdFactoryError> {
        Err(ErrorIdFactoryError::new(PrivateFailure))
    }
}

struct InternalDispatch;

impl ResolverDispatch<()> for InternalDispatch {
    fn schema_fingerprint(&self) -> chigiri_runtime::__private::ResolverSchemaFingerprint {
        super::dispatch_fingerprint()
    }

    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}

    fn resolve<'a>(
        &'a self,
        _: &'a (),
        _: &'a str,
        _: &'a str,
        _: Option<&'a ResolvedObject>,
        _: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async { Err(FieldError::internal(PrivateFailure)) })
    }
}

#[derive(Debug)]
struct PrivateFailure;

impl std::fmt::Display for PrivateFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("private failure")
    }
}

impl std::error::Error for PrivateFailure {}

#[test]
fn field_error_id_failure_is_reported_without_changing_response_semantics() {
    let recorder = Recorder::default();
    let schema = ExecutableSchema::builder(&SCHEMA, InternalDispatch)
        .error_id_factory(FailingIds)
        .error_observer(recorder.clone())
        .build()
        .unwrap();

    let response = block_on(schema.execute(GraphQlRequest::new("{ nullableError }"), ()));

    let Some(Value::Object(data)) = response.data else {
        panic!("expected partial data");
    };
    assert_eq!(data.get("nullableError"), Some(&Value::Null));
    assert!(response.errors[0].extensions.get("errorId").is_none());
    assert_factory_and_original(&recorder, "resolver");
}

#[test]
fn notifies_the_adapter_when_error_id_generation_fails() {
    let recorder = Recorder::default();
    let schema = ExecutableSchema::builder(&SCHEMA, super::Dispatch)
        .error_id_factory(FailingIds)
        .error_observer(recorder.clone())
        .build()
        .unwrap();

    let error_id = schema.observe_internal_error(
        LifecyclePhase::HttpEncode,
        "INTERNAL_SERVER_ERROR",
        &PrivateFailure,
        None,
        "response-encode",
    );

    assert!(error_id.is_none());
    assert_factory_and_original(&recorder, "response-encode");
}

fn assert_factory_and_original(recorder: &Recorder, original_component: &'static str) {
    let records = recorder.0.lock().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(
        records[0],
        (false, "ERROR_ID_GENERATION_FAILED", "error-id-factory")
    );
    assert_eq!(
        records[1],
        (false, "INTERNAL_SERVER_ERROR", original_component)
    );
}
