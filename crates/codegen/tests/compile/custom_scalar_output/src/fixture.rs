use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use chigiri::__private::{BoxFuture, ResolvedValue};
use chigiri::{
    DeadlineFactory, Endpoint, ErrorEvent, ErrorId, ErrorIdFactory, ErrorIdFactoryError,
    ErrorObserver, ExecutionLimits, FieldContext, FieldError, FieldNext, FieldResult,
    GraphQlRequest, GraphQlResponse, Object, PathSegment, Plugin, Scalar, ScalarCoercionError,
    Value,
};

use crate::generated;

pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(Waker::noop());
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

pub(crate) enum Json {
    Mode(String),
    Input(Value),
}

impl Scalar for Json {
    fn parse(value: Value) -> Result<Self, ScalarCoercionError> {
        Ok(Self::Input(value))
    }

    fn serialize(&self) -> FieldResult<Value> {
        match self {
            Self::Mode(mode) if mode == "serialize-error" => {
                Err(FieldError::internal(SerializeFailure))
            }
            Self::Mode(mode) => Ok(mode_value(mode)),
            Self::Input(value) => Ok(value.clone()),
        }
    }
}

pub(crate) struct Types;

impl generated::SchemaTypes for Types {
    type Json = Json;
}

pub(crate) struct QueryFields {
    gate: Option<Arc<TwoPartyGate>>,
}

impl generated::QueryResolver<(), Types, chigiri::FieldError> for QueryFields {
    async fn json<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        args: generated::QueryJsonArgs,
    ) -> FieldResult<Option<Json>> {
        if args.mode.starts_with("gated-") {
            self.gate
                .as_ref()
                .expect("gate must be configured")
                .wait()
                .await;
        }
        Ok(Some(Json::Mode(args.mode)))
    }

    async fn required_json<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        args: generated::QueryRequiredJsonArgs,
    ) -> FieldResult<Json> {
        Ok(Json::Mode(args.mode))
    }

    async fn echo_json<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        args: generated::QueryEchoJsonArgs<Types>,
    ) -> FieldResult<Json> {
        Ok(args.value)
    }

    async fn text<'a>(&'a self, _: chigiri::FieldContext<'a, ()>) -> FieldResult<Option<String>> {
        Ok(Some("valid".to_owned()))
    }
}

#[derive(Clone)]
pub(crate) struct Schema {
    pub(crate) endpoint: Endpoint<()>,
}

impl Schema {
    pub(crate) async fn execute(&self, request: GraphQlRequest, context: ()) -> GraphQlResponse {
        self.endpoint
            .execute(request, context, &NeverDeadline)
            .await
    }
}

pub(crate) fn schema(limits: ExecutionLimits) -> Schema {
    build(limits, None)
}

pub(crate) fn built_schema(limits: ExecutionLimits) -> Schema {
    let executable =
        chigiri::spi::ExecutableSchema::builder(&generated::SCHEMA_DESCRIPTOR, resolvers(None))
            .limits(limits)
            .build()
            .unwrap();
    let endpoint = Endpoint::builder(executable)
        .execution_limits(limits)
        .without_context()
        .build()
        .unwrap();
    Schema { endpoint }
}

pub(crate) fn gated_schema(limits: ExecutionLimits) -> Schema {
    build(limits, Some(Arc::new(TwoPartyGate::default())))
}

pub(crate) fn schema_with_response_limit(bytes: usize) -> Schema {
    let endpoint = Endpoint::builder(resolvers(None))
        .response_byte_limit(bytes)
        .without_context()
        .build()
        .unwrap();
    Schema { endpoint }
}

pub(crate) fn invalid_builtin_schema(limits: ExecutionLimits) -> Schema {
    let endpoint = Endpoint::builder(resolvers(None))
        .execution_limits(limits)
        .push_plugin(InvalidBuiltInResults)
        .without_context()
        .build()
        .unwrap();
    Schema { endpoint }
}

pub(crate) fn observed_schema(limits: ExecutionLimits) -> (Schema, Recorder) {
    let recorder = Recorder::default();
    let endpoint = Endpoint::builder(resolvers(None))
        .execution_limits(limits)
        .inject(|components| {
            components
                .observability()
                .error_id_factory(SequentialIds(AtomicU64::new(1)))
                .error_observer(recorder.clone());
        })
        .without_context()
        .build()
        .unwrap();
    (Schema { endpoint }, recorder)
}

fn build(limits: ExecutionLimits, gate: Option<Arc<TwoPartyGate>>) -> Schema {
    let endpoint = Endpoint::builder(resolvers(gate))
        .execution_limits(limits)
        .without_context()
        .build()
        .unwrap();
    Schema { endpoint }
}

fn resolvers(gate: Option<Arc<TwoPartyGate>>) -> generated::Resolvers<(), Types> {
    generated::Resolvers::builder(|_: chigiri::FieldError| -> chigiri::FieldError {
        panic!("framework scalar errors must bypass the application mapper")
    })
    .with_query(QueryFields { gate })
    .build()
}

fn mode_value(mode: &str) -> Value {
    match mode {
        "null" => Value::Null,
        "boolean" => Value::Boolean(true),
        "int" => Value::Int(42),
        "bigint" => Value::BigInt("170141183460469231731687303715884105728".to_owned()),
        "float" => Value::Float(1.5),
        "string" => Value::String("text".to_owned()),
        "object-list" => Value::Object(
            [("items", Value::List(vec![Value::Int(1), Value::Null]))]
                .into_iter()
                .collect(),
        ),
        "list-object" => Value::List(vec![Value::Object(
            [("ok", Value::Boolean(true))].into_iter().collect(),
        )]),
        "ordered" => Value::Object(
            [
                (
                    "zeta",
                    Value::List(vec![Value::String("first".to_owned()), Value::Int(2)]),
                ),
                ("alpha", Value::Null),
            ]
            .into_iter()
            .collect(),
        ),
        "nan" => Value::Object(
            [("nested", Value::List(vec![Value::Float(f64::NAN)]))]
                .into_iter()
                .collect(),
        ),
        "bad-bigint" => Value::List(vec![Value::BigInt("01-private".to_owned())]),
        "empty-object" => Value::Object(chigiri::Object::new()),
        "one-item" => Value::List(vec![Value::Boolean(true)]),
        "one-member" | "gated-left" | "gated-right" => {
            Value::Object([("value", Value::Boolean(true))].into_iter().collect())
        }
        "nested-nodes" => Value::Object(
            [(
                "a",
                Value::List(vec![Value::Object(
                    [("b", Value::Boolean(true))].into_iter().collect(),
                )]),
            )]
            .into_iter()
            .collect(),
        ),
        "root-list-3" => Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
        "nested-list-3" => Value::Object(
            [(
                "items",
                Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
            )]
            .into_iter()
            .collect(),
        ),
        "depth-2" => Value::List(vec![Value::List(vec![Value::Boolean(true)])]),
        "deep-invalid" => deep_invalid(),
        "large" => Value::Object(
            [("payload", Value::String("x".repeat(256)))]
                .into_iter()
                .collect(),
        ),
        unknown => Value::String(format!("unknown:{unknown}")),
    }
}

fn deep_invalid() -> Value {
    let mut value = Value::BigInt("01-deep-private".to_owned());
    for _ in 0..10_001 {
        value = Value::List(vec![value]);
    }
    value
}

struct InvalidBuiltInResults;

impl Plugin<()> for InvalidBuiltInResults {
    fn field<'a>(
        &'a self,
        context: FieldContext<'a, ()>,
        next: FieldNext<'a, ()>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        if context.field_name() != "text" {
            return next.run(context);
        }
        let value = match context.selection().unwrap().response_key {
            "nullValue" => Value::Null,
            "objectValue" => Value::Object(Object::new()),
            "listValue" => Value::List(Vec::new()),
            _ => Value::Int(1),
        };
        Box::pin(async move { Ok(ResolvedValue::Leaf(value)) })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Record {
    pub(crate) error_id: Option<String>,
    pub(crate) source: String,
    pub(crate) path: Vec<PathSegment>,
    pub(crate) coordinate: Option<String>,
    pub(crate) component: &'static str,
}

#[derive(Clone, Default)]
pub(crate) struct Recorder(Arc<Mutex<Vec<Record>>>);

impl Recorder {
    pub(crate) fn records(&self) -> Vec<Record> {
        self.0.lock().unwrap().clone()
    }
}

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
        ErrorId::new(format!("cso-{value}")).map_err(ErrorIdFactoryError::new)
    }
}

pub(crate) struct NeverDeadline;

impl DeadlineFactory for NeverDeadline {
    fn sleep(&self, _: std::time::Duration) -> BoxFuture<'_, ()> {
        Box::pin(std::future::pending())
    }
}

#[derive(Debug)]
struct SerializeFailure;

impl std::fmt::Display for SerializeFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("synthetic scalar serialize failure")
    }
}

impl std::error::Error for SerializeFailure {}

#[derive(Default)]
struct TwoPartyGate {
    state: Mutex<GateState>,
}

#[derive(Default)]
struct GateState {
    arrivals: usize,
    waiters: Vec<Waker>,
}

impl TwoPartyGate {
    fn wait(&self) -> GateWait<'_> {
        GateWait {
            gate: self,
            arrived: false,
        }
    }
}

struct GateWait<'a> {
    gate: &'a TwoPartyGate,
    arrived: bool,
}

impl Future for GateWait<'_> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        let mut state = self.gate.state.lock().unwrap();
        if !self.arrived {
            state.arrivals += 1;
            self.arrived = true;
        }
        if state.arrivals < 2 {
            state.waiters.push(context.waker().clone());
            return Poll::Pending;
        }
        let waiters = std::mem::take(&mut state.waiters);
        drop(state);
        for waiter in waiters {
            waiter.wake();
        }
        Poll::Ready(())
    }
}
