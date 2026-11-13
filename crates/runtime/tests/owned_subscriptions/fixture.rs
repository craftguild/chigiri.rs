use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use chigiri_runtime::__private::{
    ArgumentValues, BoxFuture, ErasedSubscription, ErasedSubscriptionArguments,
    ErasedSubscriptionEvent, ResolvedObject, ResolvedValue, SubscriptionResolverDispatch,
    erase_subscription_source, invariant_error,
};
use chigiri_runtime::core::Source;
use chigiri_runtime::core::executable::CustomScalarRegistry;
use chigiri_runtime::{
    FieldContext, FieldError, FieldResult, GraphQlRequest, OwnedResolverDispatch, OwnedSchema,
    ScalarCoercionError, SubscriptionSource, Value,
};
use futures_core::Stream;

#[derive(Default)]
pub struct Calls {
    pub setup: AtomicUsize,
    pub ordinary_root: AtomicUsize,
    pub source_drops: AtomicUsize,
    pub source_maps: AtomicUsize,
    pub fatal_queries: AtomicUsize,
    pub event_contexts: Mutex<Vec<usize>>,
}

pub struct AppContext(pub usize);

pub struct Dispatch {
    pub schema: OwnedSchema,
    pub coordinates: Vec<(Arc<str>, Arc<str>)>,
    pub root: Option<String>,
    pub register_scalar: bool,
    pub foreign_event_scope: bool,
    pub host_failure: bool,
    pub events: Vec<Result<i128, SourceFailure>>,
    pub calls: Arc<Calls>,
}

pub fn fixture() -> (OwnedSchema, Dispatch, Arc<Calls>) {
    let root = ["Owned", "Events"].concat();
    let schema = OwnedSchema::parse(vec![Source::new(
        "owned-subscriptions.graphql",
        format!(
            "scalar Probe\n\
             schema {{ query: Query mutation: Mutation subscription: {root} }}\n\
             type Query {{ echo(token: Probe!): Probe! }}\n\
             type Mutation {{ set(token: Probe!): Probe! }}\n\
             type {root} {{ events(shift: Int!, token: Probe!): Event! }}\n\
             type Event {{ value: Int! generation: Int! }}"
        ),
    )])
    .unwrap();
    let calls = Arc::new(Calls::default());
    let dispatch = Dispatch {
        schema: schema.clone(),
        coordinates: vec![
            (Arc::from("Query"), Arc::from("echo")),
            (Arc::from("Mutation"), Arc::from("set")),
            (Arc::from(root.as_str()), Arc::from("events")),
            (Arc::from("Event"), Arc::from("value")),
            (Arc::from("Event"), Arc::from("generation")),
        ],
        root: Some(root),
        register_scalar: true,
        foreign_event_scope: false,
        host_failure: false,
        events: vec![Ok(10), Ok(20)],
        calls: Arc::clone(&calls),
    };
    (schema, dispatch, calls)
}

impl OwnedResolverDispatch<AppContext> for Dispatch {
    fn schema(&self) -> &OwnedSchema {
        &self.schema
    }

    fn coordinates(&self) -> &[(Arc<str>, Arc<str>)] {
        &self.coordinates
    }

    fn register_custom_scalars(&self, registry: &mut CustomScalarRegistry) {
        if self.register_scalar {
            registry.register("Probe", |_, value| {
                if value == Value::from("reject") {
                    Err(ScalarCoercionError::public("probe rejected"))
                } else {
                    Ok(value)
                }
            });
        }
    }

    fn resolve<'a>(
        &'a self,
        context: &'a AppContext,
        parent_type: &'a str,
        field_name: &'a str,
        parent: Option<&'a ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async move {
            match (parent_type, field_name) {
                ("Query", "echo") | ("Mutation", "set") => arguments
                    .into_values()
                    .next()
                    .and_then(|value| value.into_value())
                    .map(ResolvedValue::Leaf)
                    .ok_or_else(|| invariant_error("missing finite argument")),
                ("Event", "value" | "generation") => {
                    let event = parent
                        .and_then(|parent| parent.downcast_ref::<EventObject>())
                        .ok_or_else(|| invariant_error("missing owned event object"))?;
                    assert_eq!(context.0, event.generation);
                    Ok(ResolvedValue::Leaf(Value::Int(if field_name == "value" {
                        event.value
                    } else {
                        event.generation as i128
                    })))
                }
                _ => {
                    self.calls.ordinary_root.fetch_add(1, Ordering::SeqCst);
                    Err(invariant_error("unexpected ordinary root dispatch"))
                }
            }
        })
    }
}

impl SubscriptionResolverDispatch<AppContext> for Dispatch {
    fn event_execution_failed(&self, context: &AppContext) -> bool {
        self.calls.fatal_queries.fetch_add(1, Ordering::SeqCst);
        self.host_failure && context.0 == 77
    }

    fn subscription_root(&self) -> Option<&str> {
        self.root.as_deref()
    }

    fn subscribe<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ErasedSubscription>> {
        Box::pin(async move {
            assert_eq!(context.parent_type(), self.root.as_deref().unwrap());
            assert_eq!(context.field_name(), "events");
            self.calls.setup.fetch_add(1, Ordering::SeqCst);
            let shift = match arguments.into_values().next().and_then(|v| v.into_value()) {
                Some(Value::Int(value)) => value,
                _ => return Err(invariant_error("missing coerced shift")),
            };
            let calls = Arc::clone(&self.calls);
            Ok(erase_subscription_source(
                SubscriptionSource::new(Events {
                    values: self.events.clone().into(),
                    calls: Arc::clone(&calls),
                }),
                shift,
                Arc::new(move |_: SourceFailure| {
                    calls.source_maps.fetch_add(1, Ordering::SeqCst);
                    FieldError::public("source stopped").code("SOURCE_STOPPED")
                }),
            ))
        })
    }

    fn resolve_event<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
        arguments: &'a ErasedSubscriptionArguments,
        event: &'a ErasedSubscriptionEvent,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async move {
            self.calls
                .event_contexts
                .lock()
                .unwrap()
                .push(context.context().0);
            let object = EventObject {
                value: event.downcast_ref::<i128>()? + arguments.downcast_ref::<i128>()?,
                generation: context.context().0,
            };
            let schema = if self.foreign_event_scope {
                fixture().0
            } else {
                self.schema.clone()
            };
            Ok(ResolvedValue::Object(
                schema.object("Event", object).unwrap(),
            ))
        })
    }
}

struct EventObject {
    value: i128,
    generation: usize,
}

#[derive(Clone)]
pub struct SourceFailure;

struct Events {
    values: VecDeque<Result<i128, SourceFailure>>,
    calls: Arc<Calls>,
}

impl Stream for Events {
    type Item = Result<i128, SourceFailure>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.values.pop_front())
    }
}

impl Drop for Events {
    fn drop(&mut self) {
        self.calls.source_drops.fetch_add(1, Ordering::SeqCst);
    }
}

pub fn request() -> GraphQlRequest {
    GraphQlRequest::new(
        "subscription { chosen: events(shift: 3, token: \"ok\") { value generation } }",
    )
}

pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = Box::pin(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}
