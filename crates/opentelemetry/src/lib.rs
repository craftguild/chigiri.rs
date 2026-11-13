#![forbid(unsafe_code)]
//! Opt-in owned execution instrumentation and legacy event observers.
//! Register [`OpenTelemetryInstrumentation`] once for actual context propagation.
//! Application-owned providers, processors and exporters retain their shutdown policy.

mod instrumentation;
mod spans;
#[cfg(test)]
mod test_support;

use chigiri_runtime::{
    ErrorEvent, ErrorObserver, LifecycleEvent, LifecycleObserver, LifecyclePhase, LifecycleResult,
};
use opentelemetry::trace::Tracer;
use opentelemetry::{KeyValue, StringValue};

pub use instrumentation::OpenTelemetryInstrumentation;

/// Version of the Chigiri event model emitted by this bridge.
pub const CHIGIRI_EVENT_SCHEMA_VERSION: &str = "1";
/// OpenTelemetry semantic-convention schema URL used by this bridge.
pub const OPENTELEMETRY_SCHEMA_URL: &str = opentelemetry_semantic_conventions::SCHEMA_URL;

/// Observer that creates request spans with the application-selected tracer.
#[derive(Clone, Debug)]
pub struct OpenTelemetryObserver<T> {
    spans: spans::SpanTracker<T>,
}

impl<T> OpenTelemetryObserver<T> {
    /// Injects an application-owned tracer.
    pub fn new(tracer: T) -> Self {
        Self {
            spans: spans::SpanTracker::new(tracer),
        }
    }

    /// Returns the injected tracer.
    pub const fn tracer(&self) -> &T {
        self.spans.tracer()
    }
}

impl<T> LifecycleObserver for OpenTelemetryObserver<T>
where
    T: Tracer + Send + Sync + 'static,
    T::Span: Send + Sync + 'static,
{
    fn observe(&self, event: LifecycleEvent<'_>) {
        self.spans.observe(event);
    }
}

impl<T> ErrorObserver for OpenTelemetryObserver<T>
where
    T: Tracer + Send + Sync + 'static,
    T::Span: Send + Sync + 'static,
{
    fn observe_error(&self, event: ErrorEvent<'_>) {
        self.spans.observe_error(event);
    }
}

fn lifecycle_attributes(event: LifecycleEvent<'_>) -> Vec<KeyValue> {
    let identity = event.identity();
    let mut attributes = vec![
        string("chigiri.phase", phase_name(event.phase())),
        string("chigiri.result", result_name(event.result())),
        string("chigiri.component", event.component()),
        KeyValue::new("chigiri.has_request_id", identity.request_id().is_some()),
        KeyValue::new("chigiri.has_trace_id", identity.trace_id().is_some()),
        KeyValue::new("chigiri.has_operation", identity.operation().is_some()),
    ];
    if let Some(code) = event.code() {
        attributes.push(string("chigiri.code", code));
    }
    if let Some(duration) = event.duration() {
        let duration_ns = duration.as_nanos().min(i64::MAX as u128) as i64;
        attributes.push(KeyValue::new("chigiri.duration_ns", duration_ns));
    }
    attributes
}

fn error_attributes(event: &ErrorEvent<'_>) -> Vec<KeyValue> {
    let identity = event.identity();
    vec![
        string("chigiri.phase", phase_name(event.phase())),
        string("chigiri.component", event.component()),
        string("error.type", event.code()),
        KeyValue::new("chigiri.has_error_id", event.error_id().is_some()),
        KeyValue::new("chigiri.has_request_id", identity.request_id().is_some()),
        KeyValue::new("chigiri.has_trace_id", identity.trace_id().is_some()),
        KeyValue::new("chigiri.has_operation", identity.operation().is_some()),
        KeyValue::new("chigiri.has_path", event.path().is_some()),
        KeyValue::new("chigiri.has_coordinate", event.coordinate().is_some()),
    ]
}

fn string(key: &'static str, value: &'static str) -> KeyValue {
    KeyValue::new(key, StringValue::from(value))
}

fn span_name(phase: LifecyclePhase, component: &'static str) -> &'static str {
    if component == "field" {
        return "graphql.field.resolve";
    }
    match phase {
        LifecyclePhase::RequestStarted | LifecyclePhase::RequestCompleted => "graphql.request",
        LifecyclePhase::OperationSourceResolution
        | LifecyclePhase::DocumentPreparation
        | LifecyclePhase::OperationAdmission => "graphql.operation.resolve",
        LifecyclePhase::Execution => "graphql.operation.execute",
        _ => "chigiri.lifecycle",
    }
}

fn phase_name(phase: LifecyclePhase) -> &'static str {
    match phase {
        LifecyclePhase::ServerBuilt => "server-built",
        LifecyclePhase::RequestStarted => "request-started",
        LifecyclePhase::HttpPreflight => "http-preflight",
        LifecyclePhase::TransportAdmission => "transport-admission",
        LifecyclePhase::BodyRead => "body-read",
        LifecyclePhase::HttpDecode => "http-decode",
        LifecyclePhase::OperationSourceResolution => "operation-source-resolution",
        LifecyclePhase::DecodedRequestHook => "decoded-request-hook",
        LifecyclePhase::ContextConstruction => "context-construction",
        LifecyclePhase::RequestPlugin => "request-plugin",
        LifecyclePhase::DocumentPreparation => "document-preparation",
        LifecyclePhase::OperationAdmission => "operation-admission",
        LifecyclePhase::Execution => "execution",
        LifecyclePhase::ResponseHook => "response-hook",
        LifecyclePhase::HttpEncode => "http-encode",
        LifecyclePhase::RequestCompleted => "request-completed",
    }
}

fn result_name(result: LifecycleResult) -> &'static str {
    match result {
        LifecycleResult::Started => "started",
        LifecycleResult::Succeeded => "succeeded",
        LifecycleResult::Rejected => "rejected",
        LifecycleResult::Failed => "failed",
        LifecycleResult::TimedOut => "timed-out",
        LifecycleResult::Cancelled => "cancelled",
        LifecycleResult::CacheHit => "cache-hit",
        LifecycleResult::CacheMiss => "cache-miss",
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use chigiri_runtime::{
        ErrorEvent, ErrorId, ErrorObserver, LifecycleEvent, LifecycleObserver, LifecyclePhase,
        LifecycleResult, RequestId, RequestIdentity, RequestTelemetry, SamplingDecision,
    };
    use opentelemetry::trace::SpanId;
    use opentelemetry::trace::noop::NoopTracer;

    use super::{
        OPENTELEMETRY_SCHEMA_URL, OpenTelemetryObserver, error_attributes, lifecycle_attributes,
        span_name,
    };
    use crate::test_support::{RecordingTracer, SpanRecord};

    #[test]
    fn bridge_implements_both_observer_contracts() {
        fn accepts(_: &dyn LifecycleObserver, _: &dyn ErrorObserver) {}
        let observer = OpenTelemetryObserver::new(NoopTracer::new());
        accepts(&observer, &observer);
        assert_eq!(
            OPENTELEMETRY_SCHEMA_URL,
            "https://opentelemetry.io/schemas/1.42.0"
        );
    }

    #[test]
    fn lifecycle_properties_have_no_secret_identity_or_value() {
        let request_id = RequestId::new("private-request-id").unwrap();
        let event = LifecycleEvent::new(
            LifecyclePhase::Execution,
            LifecycleResult::Succeeded,
            None,
            RequestIdentity::new(Some(&request_id), None, None),
            None,
            "field",
        );
        let debug = format!("{:?}", lifecycle_attributes(event));
        assert!(!debug.contains("private-request-id"));
        assert_eq!(
            span_name(event.phase(), event.component()),
            "graphql.field.resolve"
        );
    }

    #[test]
    fn error_attributes_exclude_sources_and_error_ids() {
        let error_id = ErrorId::new("private-error-id").unwrap();
        let source = Secret;
        let event = ErrorEvent::new(
            Some(&error_id),
            RequestIdentity::new(None, None, None),
            LifecyclePhase::Execution,
            "INTERNAL_SERVER_ERROR",
            &source,
            None,
            None,
            "resolver",
        );
        let debug = format!("{:?}", error_attributes(&event));
        assert!(!debug.contains("private-error-id"));
        assert!(!debug.contains("database-password"));
    }

    #[test]
    fn request_operation_and_execution_spans_have_the_expected_parentage() {
        let tracer = RecordingTracer::default();
        let observer = OpenTelemetryObserver::new(tracer.clone());
        let telemetry = RequestTelemetry::new(
            Some(RequestId::new("request-1").unwrap()),
            None,
            SamplingDecision::Record,
        );
        emit(
            &observer,
            &telemetry,
            LifecyclePhase::RequestStarted,
            LifecycleResult::Started,
        );
        emit(
            &observer,
            &telemetry,
            LifecyclePhase::OperationSourceResolution,
            LifecycleResult::Started,
        );
        emit(
            &observer,
            &telemetry,
            LifecyclePhase::OperationSourceResolution,
            LifecycleResult::Succeeded,
        );
        emit(
            &observer,
            &telemetry,
            LifecyclePhase::Execution,
            LifecycleResult::Started,
        );
        emit(
            &observer,
            &telemetry,
            LifecyclePhase::Execution,
            LifecycleResult::Succeeded,
        );
        emit(
            &observer,
            &telemetry,
            LifecyclePhase::RequestCompleted,
            LifecycleResult::Succeeded,
        );

        let records = tracer.records();
        let root = record(&records, "graphql.request");
        let operation = record(&records, "graphql.operation.resolve");
        let provider = record(&records, "chigiri.provider.operation-source");
        let execution = record(&records, "graphql.operation.execute");
        assert_eq!(root.parent, SpanId::INVALID);
        assert_eq!(operation.parent, root.id);
        assert_eq!(provider.parent, operation.id);
        assert_eq!(execution.parent, operation.id);
    }

    #[test]
    fn drop_sampling_excludes_only_lifecycle_and_error_is_always_recorded() {
        let tracer = RecordingTracer::default();
        let observer = OpenTelemetryObserver::new(tracer.clone());
        let telemetry = RequestTelemetry::new(
            Some(RequestId::new("request-drop").unwrap()),
            None,
            SamplingDecision::Drop,
        );
        emit(
            &observer,
            &telemetry,
            LifecyclePhase::RequestStarted,
            LifecycleResult::Started,
        );
        let source = Secret;
        observer.observe_error(ErrorEvent::new(
            None,
            telemetry.identity(None),
            LifecyclePhase::Execution,
            "INTERNAL_SERVER_ERROR",
            &source,
            None,
            None,
            "resolver",
        ));
        assert_eq!(
            tracer
                .records()
                .iter()
                .map(|record| record.name.as_str())
                .collect::<Vec<_>>(),
            ["chigiri.error"]
        );
    }

    fn emit(
        observer: &OpenTelemetryObserver<RecordingTracer>,
        telemetry: &RequestTelemetry,
        phase: LifecyclePhase,
        result: LifecycleResult,
    ) {
        observer.observe(LifecycleEvent::new(
            phase,
            result,
            None,
            telemetry.identity(None),
            None,
            "test",
        ));
    }

    fn record<'a>(records: &'a [SpanRecord], name: &str) -> &'a SpanRecord {
        records.iter().find(|record| record.name == name).unwrap()
    }

    #[derive(Debug)]
    struct Secret;

    impl std::fmt::Display for Secret {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("database-password=never-log")
        }
    }

    impl Error for Secret {}
}
