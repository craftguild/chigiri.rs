use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chigiri_runtime::__private::{
    ArgumentValues, BoxFuture, OneShotOperationKind, ResolvedObject, ResolvedValue,
};
use chigiri_runtime::core::Source;
use chigiri_runtime::core::executable::CustomScalarRegistry;
use chigiri_runtime::{
    ExecutableSchema, FieldContext, FieldError, FieldNext, FieldResult, GraphQlRequest,
    GraphQlResponse, Object, OperationContext, OperationNext, OwnedResolverDispatch, OwnedSchema,
    Plugin, RequestContext, RequestNext, ScalarCoercionError, Value,
};

#[derive(Default)]
pub struct Calls {
    pub scalar: AtomicUsize,
    pub resolver: AtomicUsize,
    pub request_plugin: AtomicUsize,
    pub operation_plugin: AtomicUsize,
    pub field_plugin: AtomicUsize,
    pub contexts: Mutex<Vec<&'static str>>,
}

pub struct AppContext {
    pub marker: &'static str,
}

pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let mut future = Box::pin(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(output) => return output,
            std::task::Poll::Pending => std::thread::yield_now(),
        }
    }
}

struct Dispatch {
    schema: OwnedSchema,
    coordinates: Vec<(Arc<str>, Arc<str>)>,
    calls: Arc<Calls>,
}

impl OwnedResolverDispatch<AppContext> for Dispatch {
    fn schema(&self) -> &OwnedSchema {
        &self.schema
    }

    fn coordinates(&self) -> &[(Arc<str>, Arc<str>)] {
        &self.coordinates
    }

    fn register_custom_scalars(&self, registry: &mut CustomScalarRegistry) {
        let calls = Arc::clone(&self.calls);
        registry.register("Probe", move |_, value| {
            calls.scalar.fetch_add(1, Ordering::SeqCst);
            if value == Value::from("reject") {
                return Err(ScalarCoercionError::public("probe rejected"));
            }
            Ok(value)
        });
    }

    fn resolve<'a>(
        &'a self,
        context: &'a AppContext,
        parent_type: &'a str,
        field_name: &'a str,
        _: Option<&'a ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.calls.resolver.fetch_add(1, Ordering::SeqCst);
        self.calls.contexts.lock().unwrap().push(context.marker);
        Box::pin(async move {
            match (parent_type, field_name) {
                ("Query", "echo") | ("Mutation", "set") => arguments
                    .into_values()
                    .next()
                    .and_then(|value| value.into_value())
                    .map(ResolvedValue::Leaf)
                    .ok_or_else(|| FieldError::public("missing prepared argument")),
                ("Query", "constant") => Ok(ResolvedValue::Leaf(Value::Int(7))),
                ("Query", "many") => Ok(ResolvedValue::Leaf(Value::Int(1))),
                ("Query", "fail") => Err(FieldError::internal(Secret("resolver-secret"))),
                ("Subscription", "ticks") => {
                    Err(FieldError::public("subscription resolver must not run"))
                }
                _ => Err(FieldError::public("unexpected coordinate")),
            }
        })
    }
}

struct Recorder(Arc<Calls>);

impl Plugin<AppContext> for Recorder {
    fn request<'a>(
        &'a self,
        context: RequestContext<'a, AppContext>,
        next: RequestNext<'a, AppContext>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        self.0.request_plugin.fetch_add(1, Ordering::SeqCst);
        next.run(context)
    }

    fn operation<'a>(
        &'a self,
        context: OperationContext<'a, AppContext>,
        next: OperationNext<'a, AppContext>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        self.0.operation_plugin.fetch_add(1, Ordering::SeqCst);
        next.run(context)
    }

    fn field<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
        next: FieldNext<'a, AppContext>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.0.field_plugin.fetch_add(1, Ordering::SeqCst);
        next.run(context)
    }
}

#[derive(Debug)]
struct Secret(&'static str);

impl std::fmt::Display for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for Secret {}

pub fn schema() -> (ExecutableSchema<AppContext>, Arc<Calls>) {
    let owned = OwnedSchema::parse(vec![Source::new(
        "prepared-one-shot.graphql",
        "scalar Probe\n\
         schema { query: Query mutation: Mutation subscription: Subscription }\n\
         type Query { echo(value: Probe!): Probe! many(values: [Int!]!): Int! constant: Int! fail: String }\n\
         type Mutation { set(value: Probe!): Probe! }\n\
         type Subscription { ticks(value: Probe!): String! }",
    )])
    .unwrap();
    let calls = Arc::new(Calls::default());
    let dispatch = Dispatch {
        schema: owned.clone(),
        coordinates: vec![
            (Arc::from("Query"), Arc::from("echo")),
            (Arc::from("Query"), Arc::from("constant")),
            (Arc::from("Query"), Arc::from("many")),
            (Arc::from("Query"), Arc::from("fail")),
            (Arc::from("Mutation"), Arc::from("set")),
            (Arc::from("Subscription"), Arc::from("ticks")),
        ],
        calls: Arc::clone(&calls),
    };
    let schema = owned
        .bind(dispatch)
        .unwrap()
        .into_executable()
        .reconfigure()
        .plugin(Recorder(Arc::clone(&calls)))
        .build();
    calls.scalar.store(0, Ordering::SeqCst);
    (schema, calls)
}

pub fn named(kind: &str, field: &str, value: impl Into<Value>) -> GraphQlRequest {
    let mut variables = Object::new();
    variables.insert("value", value);
    GraphQlRequest::new(format!(
        "{kind} Named($value: Probe!) {{ {field}(value: $value) }}"
    ))
    .with_operation_name("Named")
    .with_variables(variables)
}

pub fn many(count: usize) -> GraphQlRequest {
    let mut variables = Object::new();
    variables.insert(
        "values",
        Value::List((0..count).map(|_| Value::Int(1)).collect()),
    );
    GraphQlRequest::new("query Named($values: [Int!]!) { many(values: $values) }")
        .with_operation_name("Named")
        .with_variables(variables)
}

pub fn assert_kind(kind: OneShotOperationKind, expected: OneShotOperationKind) {
    assert_eq!(kind, expected);
}

pub fn code(response: &GraphQlResponse) -> Option<&str> {
    response
        .errors
        .first()
        .and_then(|error| error.extensions.get("code"))
        .and_then(Value::as_str)
}

pub fn object_leaf<'a>(response: &'a GraphQlResponse, field: &str) -> Option<&'a Value> {
    response
        .data
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|object| object.get(field))
}

pub fn scalar_calls(calls: &Calls) -> usize {
    calls.scalar.load(Ordering::SeqCst)
}

pub fn resolver_calls(calls: &Calls) -> usize {
    calls.resolver.load(Ordering::SeqCst)
}
