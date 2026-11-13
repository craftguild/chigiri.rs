use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll, Waker};

use crate::executable::CoercedInput;
use crate::executor::{
    ArgumentValues, BoxFuture, FieldContext, FieldDispatch, FieldNext, ObjectScope,
    OperationContext, OperationNext, Plugin, ResolvedObject, ResolvedValue,
    SubscriptionEventDispatch, SubscriptionSetupDispatch,
};
use crate::{FieldError, FieldResult, LifecycleEvent, LifecycleObserver, Value};

#[derive(Clone, Copy)]
pub(super) struct TestContext {
    pub generation: usize,
}

pub(super) struct EventRow {
    pub required: &'static str,
    pub label: &'static str,
}

pub(super) struct SubscriptionMarker;

pub(super) struct TestDispatch {
    pub root_calls: AtomicUsize,
    pub nested_calls: AtomicUsize,
    pub fail_required: bool,
    pub byte_limit: Option<usize>,
}

impl TestDispatch {
    pub fn new(fail_required: bool) -> Self {
        Self {
            root_calls: AtomicUsize::new(0),
            nested_calls: AtomicUsize::new(0),
            fail_required,
            byte_limit: None,
        }
    }

    pub fn with_byte_limit(mut self, byte_limit: usize) -> Self {
        self.byte_limit = Some(byte_limit);
        self
    }
}

impl FieldDispatch<TestContext> for TestDispatch {
    fn object_scope(&self) -> Option<&ObjectScope> {
        None
    }

    fn runtime_byte_limit(&self) -> Option<usize> {
        self.byte_limit
    }

    fn resolve<'a>(
        &'a self,
        _context: &'a TestContext,
        parent_type: &'a str,
        field_name: &'a str,
        parent: Option<&'a ResolvedObject>,
        _arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        if parent_type == "Query" && field_name == "nested" {
            return Box::pin(async {
                Ok(ResolvedValue::object("Subscription", SubscriptionMarker))
            });
        }
        if parent_type == "Subscription" {
            self.root_calls.fetch_add(1, Ordering::SeqCst);
            if parent.is_some() {
                return Box::pin(async {
                    Ok(ResolvedValue::object(
                        "Event",
                        EventRow {
                            required: "nested",
                            label: "ordinary",
                        },
                    ))
                });
            }
            return Box::pin(async {
                Err(FieldError::public(
                    "ordinary subscription-root dispatch must not run",
                ))
            });
        }
        self.nested_calls.fetch_add(1, Ordering::SeqCst);
        let row = parent.unwrap().downcast_ref::<EventRow>().unwrap();
        let result = match field_name {
            "required" if self.fail_required => Err(FieldError::internal(TestFailure)),
            "required" => Ok(ResolvedValue::Leaf(Value::String(row.required.to_owned()))),
            "label" => Ok(ResolvedValue::Leaf(Value::String(row.label.to_owned()))),
            _ => panic!("unexpected field {parent_type}.{field_name}"),
        };
        Box::pin(async move { result })
    }
}

#[derive(Debug)]
struct TestFailure;

impl std::fmt::Display for TestFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("private nested failure")
    }
}

impl std::error::Error for TestFailure {}

pub(super) struct Setup {
    pub calls: AtomicUsize,
    pub contexts: Mutex<Vec<usize>>,
}

impl Setup {
    pub fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            contexts: Mutex::new(Vec::new()),
        }
    }
}

impl SubscriptionSetupDispatch<TestContext, String> for Setup {
    fn subscribe<'a>(
        &'a self,
        context: FieldContext<'a, TestContext>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<String>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.contexts
            .lock()
            .unwrap()
            .push(context.context().generation);
        assert_eq!(context.parent_type(), "Subscription");
        assert_eq!(context.field_name(), "event");
        assert_eq!(
            context.arguments().unwrap().get("id"),
            Some(&Value::from("room"))
        );
        let values = arguments.into_values().collect::<Vec<_>>();
        assert_eq!(values, [CoercedInput::Value(Value::from("room"))]);
        Box::pin(async { Ok("session".to_owned()) })
    }
}

pub(super) struct Event {
    pub calls: AtomicUsize,
    pub contexts: Mutex<Vec<usize>>,
    pub required: &'static str,
    pub label: &'static str,
}

impl Event {
    pub fn new(required: &'static str, label: &'static str) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            contexts: Mutex::new(Vec::new()),
            required,
            label,
        }
    }
}

impl SubscriptionEventDispatch<TestContext> for Event {
    fn resolve_event<'a>(
        &'a self,
        context: FieldContext<'a, TestContext>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.contexts
            .lock()
            .unwrap()
            .push(context.context().generation);
        let row = EventRow {
            required: self.required,
            label: self.label,
        };
        Box::pin(async move { Ok(ResolvedValue::object("Event", row)) })
    }
}

pub(super) struct RecordingPlugin {
    pub operations: Mutex<Vec<(usize, usize, usize)>>,
    pub fields: Mutex<Vec<(usize, String, usize)>>,
}

impl RecordingPlugin {
    pub fn new() -> Self {
        Self {
            operations: Mutex::new(Vec::new()),
            fields: Mutex::new(Vec::new()),
        }
    }
}

impl Plugin<TestContext> for RecordingPlugin {
    fn operation<'a>(
        &'a self,
        context: OperationContext<'a, TestContext>,
        next: OperationNext<'a, TestContext>,
    ) -> BoxFuture<'a, crate::GraphQlResponse> {
        self.operations.lock().unwrap().push((
            context.context().generation,
            context.directives().len(),
            context.cost_fields().len(),
        ));
        next.run(context)
    }

    fn field<'a>(
        &'a self,
        context: FieldContext<'a, TestContext>,
        next: FieldNext<'a, TestContext>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.fields.lock().unwrap().push((
            context.context().generation,
            format!("{}.{}", context.parent_type(), context.field_name()),
            context.directives().len(),
        ));
        next.run(context)
    }
}

pub(super) struct LifecycleLog(pub Mutex<Vec<(crate::LifecyclePhase, crate::LifecycleResult)>>);

impl LifecycleObserver for LifecycleLog {
    fn observe(&self, event: LifecycleEvent<'_>) {
        self.0.lock().unwrap().push((event.phase(), event.result()));
    }
}

pub(super) struct PendingEvent {
    pub dropped: Arc<AtomicBool>,
}

impl Future for PendingEvent {
    type Output = FieldResult<ResolvedValue>;

    fn poll(self: std::pin::Pin<&mut Self>, _context: &mut TaskContext<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

impl Drop for PendingEvent {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

pub(super) fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = TaskContext::from_waker(Waker::noop());
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}
