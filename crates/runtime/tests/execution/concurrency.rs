//! Tests bounded query concurrency and serial public execution of mutation roots.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use chigiri_runtime::__private::{ArgumentValues, BoxFuture, ResolvedValue, ResolverDispatch};
use chigiri_runtime::core::executable::CustomScalarRegistry;
use chigiri_runtime::{
    ExecutableSchema, ExecutionLimits, FieldContext, FieldError, FieldNext, FieldResult,
    GraphQlRequest, Plugin, Value,
};

use super::SCHEMA;

#[test]
fn query_siblings_poll_concurrently_and_preserve_source_order_despite_reverse_completion() {
    let state = Arc::new(Mutex::new(PairState::default()));
    let schema = ExecutableSchema::builder(&SCHEMA, PairDispatch(Arc::clone(&state)))
        .build()
        .unwrap();
    let response = block_on(schema.execute(
        GraphQlRequest::new(r#"{ slow: echo(value: "slow"), fast: echo(value: "fast") }"#),
        (),
    ));
    let Some(Value::Object(data)) = response.data else {
        panic!("expected object data");
    };

    assert!(response.errors.is_empty());
    assert_eq!(
        data.iter().map(|(key, _)| key).collect::<Vec<_>>(),
        ["slow", "fast"]
    );
    assert!(state.lock().unwrap().both_polled);
}

#[test]
fn field_plugins_are_loaded_safely_during_concurrent_execution() {
    let state = Arc::new(Mutex::new(PairState::default()));
    let active = Arc::new(AtomicUsize::new(0));
    let maximum = Arc::new(AtomicUsize::new(0));
    let schema = ExecutableSchema::builder(&SCHEMA, PairDispatch(Arc::clone(&state)))
        .plugin(ConcurrentPlugin {
            active: Arc::clone(&active),
            maximum: Arc::clone(&maximum),
        })
        .build()
        .unwrap();

    let response = block_on(schema.execute(
        GraphQlRequest::new(r#"{ slow: echo(value: "slow"), fast: echo(value: "fast") }"#),
        (),
    ));

    assert!(response.errors.is_empty());
    assert_eq!(maximum.load(Ordering::SeqCst), 2);
    assert_eq!(active.load(Ordering::SeqCst), 0);
}

#[test]
fn concurrent_fields_do_not_start_resolver_futures_beyond_the_limit() {
    let active = Arc::new(AtomicUsize::new(0));
    let maximum = Arc::new(AtomicUsize::new(0));
    let dispatch = LimitDispatch {
        active: Arc::clone(&active),
        maximum: Arc::clone(&maximum),
    };
    let mut limits = ExecutionLimits::production();
    limits.max_concurrent_fields = 1;
    let schema = ExecutableSchema::builder(&SCHEMA, dispatch)
        .limits(limits)
        .build()
        .unwrap();

    let response = block_on(schema.execute(
        GraphQlRequest::new(r#"{ a: echo(value: "a"), b: echo(value: "b") }"#),
        (),
    ));

    assert!(response.errors.is_empty());
    assert_eq!(maximum.load(Ordering::SeqCst), 1);
    assert_eq!(active.load(Ordering::SeqCst), 0);
}

#[test]
fn mutation_roots_start_each_resolver_after_the_previous_field_completes() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let schema = ExecutableSchema::builder(&SCHEMA, SerialDispatch(Arc::clone(&events)))
        .build()
        .unwrap();

    let response = block_on(schema.execute(
        GraphQlRequest::new(r#"mutation { a: set(value: "a"), b: set(value: "b") }"#),
        (),
    ));

    assert!(response.errors.is_empty());
    assert_eq!(
        *events.lock().unwrap(),
        ["start a", "finish a", "start b", "finish b"]
    );
}

#[derive(Default)]
pub(super) struct PairState {
    pub(super) slow_polled: bool,
    pub(super) fast_polled: bool,
    pub(super) fast_done: bool,
    pub(super) both_polled: bool,
}

struct PairDispatch(Arc<Mutex<PairState>>);

impl ResolverDispatch<()> for PairDispatch {
    fn schema_fingerprint(&self) -> chigiri_runtime::__private::ResolverSchemaFingerprint {
        super::dispatch_fingerprint()
    }

    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}

    fn resolve<'a>(
        &'a self,
        _: &'a (),
        parent_type: &'a str,
        field_name: &'a str,
        _: Option<&'a chigiri_runtime::__private::ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        if (parent_type, field_name) != ("Query", "echo") {
            return Box::pin(async { Err(FieldError::public("unexpected field")) });
        }
        let value = argument_string(arguments);
        Box::pin(PairFuture {
            value,
            state: Arc::clone(&self.0),
        })
    }
}

pub(super) struct PairFuture {
    pub(super) value: String,
    pub(super) state: Arc<Mutex<PairState>>,
}

impl Future for PairFuture {
    type Output = FieldResult<ResolvedValue>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.state.lock().unwrap();
        if self.value == "slow" {
            state.slow_polled = true;
        } else {
            state.fast_polled = true;
        }
        state.both_polled = state.slow_polled && state.fast_polled;
        let ready = state.both_polled && (self.value == "fast" || state.fast_done);
        if ready {
            if self.value == "fast" {
                state.fast_done = true;
            }
            Poll::Ready(Ok(ResolvedValue::Leaf(Value::String(self.value.clone()))))
        } else {
            context.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

struct LimitDispatch {
    active: Arc<AtomicUsize>,
    maximum: Arc<AtomicUsize>,
}

struct ConcurrentPlugin {
    active: Arc<AtomicUsize>,
    maximum: Arc<AtomicUsize>,
}

impl Plugin<()> for ConcurrentPlugin {
    fn field<'a>(
        &'a self,
        context: FieldContext<'a, ()>,
        next: FieldNext<'a, ()>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async move {
            let current = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum.fetch_max(current, Ordering::SeqCst);
            let result = next.run(context).await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            result
        })
    }
}

impl ResolverDispatch<()> for LimitDispatch {
    fn schema_fingerprint(&self) -> chigiri_runtime::__private::ResolverSchemaFingerprint {
        super::dispatch_fingerprint()
    }

    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}

    fn resolve<'a>(
        &'a self,
        _: &'a (),
        _: &'a str,
        _: &'a str,
        _: Option<&'a chigiri_runtime::__private::ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        let current = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum.fetch_max(current, Ordering::SeqCst);
        Box::pin(LimitFuture {
            value: argument_string(arguments),
            active: Arc::clone(&self.active),
            pending: true,
        })
    }
}

struct LimitFuture {
    value: String,
    active: Arc<AtomicUsize>,
    pending: bool,
}

impl Future for LimitFuture {
    type Output = FieldResult<ResolvedValue>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if self.pending {
            self.pending = false;
            context.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(Ok(ResolvedValue::Leaf(Value::String(self.value.clone()))))
        }
    }
}

impl Drop for LimitFuture {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::SeqCst);
    }
}

struct SerialDispatch(Arc<Mutex<Vec<String>>>);

impl ResolverDispatch<()> for SerialDispatch {
    fn schema_fingerprint(&self) -> chigiri_runtime::__private::ResolverSchemaFingerprint {
        super::dispatch_fingerprint()
    }

    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}

    fn resolve<'a>(
        &'a self,
        _: &'a (),
        _: &'a str,
        _: &'a str,
        _: Option<&'a chigiri_runtime::__private::ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        let value = argument_string(arguments);
        self.0.lock().unwrap().push(format!("start {value}"));
        Box::pin(async move {
            self.0.lock().unwrap().push(format!("finish {value}"));
            Ok(ResolvedValue::Leaf(Value::String(value)))
        })
    }
}

fn argument_string(arguments: ArgumentValues) -> String {
    let value = arguments
        .into_values()
        .next()
        .and_then(|value| value.into_value())
        .unwrap();
    let Value::String(value) = value else {
        panic!("expected a string argument");
    };
    value
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(Waker::noop());
    let mut future = Box::pin(future);
    for _ in 0..10_000 {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
    }
    panic!("future was not completed within the number of times bounded poll was completed.");
}
