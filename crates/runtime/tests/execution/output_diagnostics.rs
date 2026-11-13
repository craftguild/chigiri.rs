//! Tests correlation of completion resource limits with error IDs and observer sources.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chigiri_runtime::{
    ErrorEvent, ErrorId, ErrorIdFactory, ErrorIdFactoryError, ErrorObserver, ExecutableSchema,
    ExecutionLimits, GraphQlRequest, GraphQlResponse, IntrospectionPolicy, PathSegment, Value,
};

use super::{Dispatch, SCHEMA, block_on};

#[derive(Clone, Debug, Eq, PartialEq)]
struct Record {
    error_id: Option<String>,
    source: String,
    path: Vec<PathSegment>,
    coordinate: Option<String>,
    component: &'static str,
}

#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<Record>>>);

impl ErrorObserver for Recorder {
    fn observe_error(&self, event: ErrorEvent<'_>) {
        self.0.lock().unwrap().push(Record {
            error_id: event.error_id().map(|id| id.as_str().to_owned()),
            source: event.source().to_string(),
            path: event.path().unwrap_or_default().to_vec(),
            coordinate: event.coordinate().map(str::to_owned),
            component: event.component(),
        });
    }
}

struct SequentialIds(AtomicU64);

impl ErrorIdFactory for SequentialIds {
    fn create(&self) -> Result<ErrorId, ErrorIdFactoryError> {
        let value = self.0.fetch_add(1, Ordering::SeqCst);
        ErrorId::new(format!("output-{value}")).map_err(ErrorIdFactoryError::new)
    }
}

#[test]
fn direct_field_list_errors_are_correlated_with_masked_ids_and_sources() {
    let mut limits = ExecutionLimits::production();
    limits.max_list_items_per_field = 2;
    let (schema, recorder) = observed_schema(limits);
    let response = block_on(schema.execute(GraphQlRequest::new("{ nullableItems }"), ()));

    assert_correlated(
        response,
        &recorder,
        &[PathSegment::from("nullableItems")],
        "Query.nullableItems",
        "completion",
    );
}

#[test]
fn direct_output_node_errors_are_correlated_with_masked_ids_and_sources() {
    let mut limits = ExecutionLimits::production();
    limits.max_output_nodes = 1;
    let (schema, recorder) = observed_schema(limits);
    let response = block_on(schema.execute(GraphQlRequest::new("{ item { name } }"), ()));

    assert_correlated(
        response,
        &recorder,
        &[PathSegment::from("item"), PathSegment::from("name")],
        "Item.name",
        "completion",
    );
}

#[test]
fn introspection_output_node_limits_correlate_with_common_diagnostics() {
    let mut limits = ExecutionLimits::production();
    limits.max_output_nodes = 1;
    let recorder = Recorder::default();
    let schema = ExecutableSchema::builder(&SCHEMA, Dispatch)
        .limits(limits)
        .introspection(IntrospectionPolicy::Enabled)
        .error_id_factory(SequentialIds(AtomicU64::new(1)))
        .error_observer(recorder.clone())
        .build()
        .unwrap();
    let response = block_on(schema.execute(
        GraphQlRequest::new("{ __schema { queryType { name } } }"),
        (),
    ));

    assert_correlated(
        response,
        &recorder,
        &[
            PathSegment::from("__schema"),
            PathSegment::from("queryType"),
        ],
        "__Schema.queryType",
        "introspection",
    );
}

fn observed_schema(limits: ExecutionLimits) -> (ExecutableSchema<()>, Recorder) {
    let recorder = Recorder::default();
    let schema = ExecutableSchema::builder(&SCHEMA, Dispatch)
        .limits(limits)
        .error_id_factory(SequentialIds(AtomicU64::new(1)))
        .error_observer(recorder.clone())
        .build()
        .unwrap();
    (schema, recorder)
}

fn assert_correlated(
    response: GraphQlResponse,
    recorder: &Recorder,
    expected_path: &[PathSegment],
    expected_coordinate: &str,
    expected_component: &'static str,
) {
    let error = &response.errors[0];
    let response_id = error
        .extensions
        .get("errorId")
        .and_then(Value::as_str)
        .unwrap();
    assert_eq!(error.message, "Internal server error");
    assert_eq!(error.path, expected_path);
    assert!(!format!("{response:?}").contains("response output limit exceeded"));

    let records = recorder.0.lock().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].error_id.as_deref(), Some(response_id));
    assert_eq!(records[0].source, "response output limit exceeded");
    assert_eq!(records[0].path, expected_path);
    assert_eq!(records[0].coordinate.as_deref(), Some(expected_coordinate));
    assert_eq!(records[0].component, expected_component);
}
