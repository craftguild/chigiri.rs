//! Test three-phase plugin order, context, and short-circuiting through the public API.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chigiri_runtime::__private::{
    ArgumentValues, BoxFuture, ResolvedObject, ResolvedValue, ResolverDispatch,
};
use chigiri_runtime::core::executable::CustomScalarRegistry;
use chigiri_runtime::{
    ExecutableSchema, FieldContext, FieldError, FieldNext, FieldResult, GraphQlRequest,
    GraphQlResponse, OperationContext, OperationNext, PathSegment, Plugin, RequestContext,
    RequestNext, Value,
};

use super::{Dispatch, SCHEMA, block_on};

struct Recorder {
    name: &'static str,
    events: Arc<Mutex<Vec<String>>>,
}

impl Plugin<()> for Recorder {
    fn request<'a>(
        &'a self,
        context: RequestContext<'a, ()>,
        next: RequestNext<'a, ()>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        Box::pin(async move {
            self.record("request enter");
            let response = next.run(context).await;
            self.record("request exit");
            response
        })
    }

    fn operation<'a>(
        &'a self,
        context: OperationContext<'a, ()>,
        next: OperationNext<'a, ()>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        Box::pin(async move {
            assert_eq!(context.operation_name(), Some("Named"));
            assert_eq!(
                context
                    .variables()
                    .get("value")
                    .and_then(|value| value.clone().into_value()),
                Some(Value::String("input".to_owned()))
            );
            self.record("operation enter");
            let response = next.run(context).await;
            self.record("operation exit");
            response
        })
    }

    fn field<'a>(
        &'a self,
        context: FieldContext<'a, ()>,
        next: FieldNext<'a, ()>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async move {
            assert_eq!(context.parent_type(), "Query");
            assert_eq!(context.field_name(), "echo");
            assert_eq!(context.path(), [PathSegment::Field("result".to_owned())]);
            assert_eq!(
                context
                    .arguments()
                    .and_then(|arguments| arguments.get("value")),
                Some(&Value::String("input".to_owned()))
            );
            self.record("field enter");
            let response = next.run(context).await;
            self.record("field exit");
            response
        })
    }
}

impl Recorder {
    fn record(&self, phase: &str) {
        self.events
            .lock()
            .unwrap()
            .push(format!("{} {phase}", self.name));
    }
}

#[test]
fn middleware_enters_in_order_exits_in_reverse_and_releases_coerced_context() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let schema = ExecutableSchema::builder(&SCHEMA, Dispatch)
        .plugin(recorder("A", &events))
        .plugin(recorder("B", &events))
        .build()
        .unwrap();
    let mut variables = chigiri_runtime::Object::new();
    variables.insert("value", "input");
    let request =
        GraphQlRequest::new("query Named($value: String!) { result: echo(value: $value) }")
            .with_variables(variables);

    let response = block_on(schema.execute(request, ()));

    assert!(response.errors.is_empty());
    assert_eq!(
        *events.lock().unwrap(),
        [
            "A request enter",
            "B request enter",
            "A operation enter",
            "B operation enter",
            "A field enter",
            "B field enter",
            "B field exit",
            "A field exit",
            "B operation exit",
            "A operation exit",
            "B request exit",
            "A request exit",
        ]
    );
}

struct ShortCircuit {
    phase: Phase,
}

enum Phase {
    Request,
    Operation,
    Field,
}

impl Plugin<()> for ShortCircuit {
    fn request<'a>(
        &'a self,
        context: RequestContext<'a, ()>,
        next: RequestNext<'a, ()>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        if matches!(self.phase, Phase::Request) {
            Box::pin(async { GraphQlResponse::executed(Value::String("request short".to_owned())) })
        } else {
            next.run(context)
        }
    }

    fn operation<'a>(
        &'a self,
        context: OperationContext<'a, ()>,
        next: OperationNext<'a, ()>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        if matches!(self.phase, Phase::Operation) {
            Box::pin(async {
                GraphQlResponse::executed(Value::String("operation short".to_owned()))
            })
        } else {
            next.run(context)
        }
    }

    fn field<'a>(
        &'a self,
        context: FieldContext<'a, ()>,
        next: FieldNext<'a, ()>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        if matches!(self.phase, Phase::Field) {
            Box::pin(async { Ok(ResolvedValue::Leaf(Value::String("field short".to_owned()))) })
        } else {
            next.run(context)
        }
    }
}

#[test]
fn each_phase_can_short_circuit_its_inner_chain() {
    let request_schema = ExecutableSchema::builder(&SCHEMA, Dispatch)
        .plugin(ShortCircuit {
            phase: Phase::Request,
        })
        .build()
        .unwrap();
    let operation_schema = ExecutableSchema::builder(&SCHEMA, Dispatch)
        .plugin(ShortCircuit {
            phase: Phase::Operation,
        })
        .build()
        .unwrap();
    let field_schema = ExecutableSchema::builder(&SCHEMA, Dispatch)
        .plugin(ShortCircuit {
            phase: Phase::Field,
        })
        .build()
        .unwrap();

    let request = block_on(request_schema.execute(GraphQlRequest::new("invalid"), ()));
    let operation = block_on(operation_schema.execute(GraphQlRequest::new("{ hello }"), ()));
    let field = block_on(field_schema.execute(GraphQlRequest::new("{ hello }"), ()));

    assert_eq!(
        request.data,
        Some(Value::String("request short".to_owned()))
    );
    assert_eq!(
        operation.data,
        Some(Value::String("operation short".to_owned()))
    );
    let Some(Value::Object(field_data)) = field.data else {
        panic!("expected a field short-circuit response object");
    };
    assert_eq!(
        field_data.get("hello"),
        Some(&Value::String("field short".to_owned()))
    );
}

#[test]
fn only_a_field_plugin_can_observe_the_internal_source_and_convert_the_result() {
    let observed = Arc::new(AtomicBool::new(false));
    let masked_schema = ExecutableSchema::builder(&SCHEMA, InternalDispatch)
        .build()
        .unwrap();
    let recovered_schema = ExecutableSchema::builder(&SCHEMA, InternalDispatch)
        .plugin(RecoverInternal(Arc::clone(&observed)))
        .build()
        .unwrap();

    let masked = block_on(masked_schema.execute(GraphQlRequest::new("{ hello }"), ()));
    let recovered = block_on(recovered_schema.execute(GraphQlRequest::new("{ hello }"), ()));

    assert_eq!(masked.errors[0].message, "Internal server error");
    assert!(!format!("{:?}", masked.errors[0]).contains("secret details"));
    assert!(observed.load(Ordering::SeqCst));
    assert!(recovered.errors.is_empty());
    let Some(Value::Object(data)) = recovered.data else {
        panic!("expected recovered object data");
    };
    assert_eq!(data.get("hello"), Some(&Value::from("recovered")));
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
        Box::pin(async { Err(FieldError::internal(SecretError)) })
    }
}

#[derive(Debug)]
struct SecretError;

impl std::fmt::Display for SecretError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("secret details")
    }
}

impl std::error::Error for SecretError {}

struct RecoverInternal(Arc<AtomicBool>);

impl Plugin<()> for RecoverInternal {
    fn field<'a>(
        &'a self,
        context: FieldContext<'a, ()>,
        next: FieldNext<'a, ()>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async move {
            match next.run(context).await {
                Err(error) if error.is_internal() => {
                    let source = error.internal_source().unwrap();
                    assert_eq!(source.to_string(), "secret details");
                    self.0.store(true, Ordering::SeqCst);
                    Ok(ResolvedValue::Leaf(Value::from("recovered")))
                }
                result => result,
            }
        })
    }
}

fn recorder(name: &'static str, events: &Arc<Mutex<Vec<String>>>) -> Recorder {
    Recorder {
        name,
        events: Arc::clone(events),
    }
}
