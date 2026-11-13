use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::{Context, Poll};

use chigiri_runtime::__private::{
    ArgumentValues, BoxFuture, ErasedSubscription, ErasedSubscriptionArguments,
    ErasedSubscriptionEvent, ResolvedObject, ResolvedValue, ResolverDispatch,
    ResolverSchemaFingerprint, SubscriptionResolverDispatch, descriptor_fingerprint,
    erase_subscription_source, invariant_error,
};
use chigiri_runtime::core::executable::CustomScalarRegistry;
use chigiri_runtime::{
    FieldContext, FieldError, FieldResult, SubscriptionOverflow, SubscriptionSource, Value,
};
use futures_core::Stream;

use super::descriptor::SCHEMA;
use super::state::{AppContext, EventMode, SetupMode, SourceStep, State};

pub struct Dispatch(pub Arc<State>);

impl ResolverDispatch<AppContext> for Dispatch {
    fn schema_fingerprint(&self) -> ResolverSchemaFingerprint {
        descriptor_fingerprint(&SCHEMA)
    }

    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}

    fn resolve<'a>(
        &'a self,
        context: &'a AppContext,
        parent_type: &'a str,
        field_name: &'a str,
        parent: Option<&'a ResolvedObject>,
        _: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        if parent_type == "Subscription" {
            self.0
                .counts
                .ordinary_root_calls
                .fetch_add(1, Ordering::SeqCst);
            return Box::pin(async { Err(invariant_error("ordinary root dispatch ran")) });
        }
        self.0.counts.nested_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            let event = parent
                .and_then(|parent| parent.downcast_ref::<EventObject>())
                .ok_or_else(|| invariant_error("event parent is absent"))?;
            match (parent_type, field_name) {
                ("Event", "value") => {
                    assert_eq!(context.generation, event.generation);
                    Ok(ResolvedValue::Leaf(Value::Int(i128::from(event.value))))
                }
                ("Event", "optionalFail" | "requiredFail") => {
                    Err(FieldError::public("ordinary nested failure").code("NESTED_FAILURE"))
                }
                ("Query", "ping") => Ok(ResolvedValue::Leaf(Value::from("pong"))),
                _ => Err(invariant_error("unexpected ordinary coordinate")),
            }
        })
    }
}

impl SubscriptionResolverDispatch<AppContext> for Dispatch {
    fn subscription_root(&self) -> Option<&'static str> {
        Some("Subscription")
    }

    fn subscribe<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ErasedSubscription>> {
        self.0.counts.setup_calls.fetch_add(1, Ordering::SeqCst);
        self.0
            .setup_generations
            .lock()
            .unwrap()
            .push(context.context().generation);
        if self.0.setup_mode == SetupMode::Pending {
            return Box::pin(PendingSetup(Arc::clone(&self.0)));
        }
        let state = Arc::clone(&self.0);
        Box::pin(async move {
            if state.setup_mode == SetupMode::OuterError {
                state.counts.outer_maps.fetch_add(1, Ordering::SeqCst);
                return Err(FieldError::public("outer setup failure").code("OUTER_FAILURE"));
            }
            let limit = typed_limit(arguments)?;
            let steps = state.source.lock().unwrap().clone();
            let stream = ScriptedStream {
                state: Arc::clone(&state),
                steps: VecDeque::from(steps),
            };
            let mapper_state = Arc::clone(&state);
            let mapper = Arc::new(move |_: SourceFailure| {
                mapper_state
                    .counts
                    .source_maps
                    .fetch_add(1, Ordering::SeqCst);
                FieldError::internal(SecretFailure("source-secret"))
            });
            Ok(erase_subscription_source(
                SubscriptionSource::new(stream).with_overflow(SubscriptionOverflow::DropOldest),
                Args { limit },
                mapper,
            ))
        })
    }

    fn resolve_event<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
        arguments: &'a ErasedSubscriptionArguments,
        event: &'a ErasedSubscriptionEvent,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.0
            .counts
            .event_dispatches
            .fetch_add(1, Ordering::SeqCst);
        let args = arguments.downcast_ref::<Args>();
        let event = event.downcast_ref::<Event>();
        let state = Arc::clone(&self.0);
        if state.event_mode == EventMode::Pending {
            return Box::pin(PendingEvent(state));
        }
        Box::pin(async move {
            let args = args?;
            let event = event?;
            state
                .event_generations
                .lock()
                .unwrap()
                .push(context.context().generation);
            state.retained_arguments.lock().unwrap().push(args.limit);
            if state.event_mode == EventMode::EventError {
                state.counts.event_maps.fetch_add(1, Ordering::SeqCst);
                return Err(FieldError::internal(SecretFailure("event-secret")));
            }
            if state.event_mode == EventMode::FrameworkInvariant {
                return Err(invariant_error("generated event invariant"));
            }
            Ok(ResolvedValue::object(
                "Event",
                EventObject {
                    value: event.value,
                    generation: context.context().generation,
                },
            ))
        })
    }
}

fn typed_limit(arguments: ArgumentValues) -> FieldResult<i32> {
    let value = arguments
        .into_values()
        .next()
        .and_then(|value| value.into_value());
    match value {
        Some(Value::Int(value)) => {
            i32::try_from(value).map_err(|_| invariant_error("limit is outside i32"))
        }
        _ => Err(invariant_error("limit is not a coerced Int")),
    }
}

struct Args {
    limit: i32,
}

struct Event {
    value: i32,
    state: Arc<State>,
}

impl Drop for Event {
    fn drop(&mut self) {
        self.state.counts.event_drops.fetch_add(1, Ordering::SeqCst);
    }
}

struct EventObject {
    value: i32,
    generation: usize,
}

#[derive(Debug)]
struct SourceFailure;

struct ScriptedStream {
    state: Arc<State>,
    steps: VecDeque<SourceStep>,
}

impl Stream for ScriptedStream {
    type Item = Result<Event, SourceFailure>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.state
            .counts
            .source_polls
            .fetch_add(1, Ordering::SeqCst);
        match self.steps.front().copied() {
            Some(SourceStep::Pending) => Poll::Pending,
            Some(SourceStep::Event(value)) => {
                self.steps.pop_front();
                Poll::Ready(Some(Ok(Event {
                    value,
                    state: Arc::clone(&self.state),
                })))
            }
            Some(SourceStep::Error) => {
                self.steps.pop_front();
                Poll::Ready(Some(Err(SourceFailure)))
            }
            None => Poll::Ready(None),
        }
    }
}

impl Drop for ScriptedStream {
    fn drop(&mut self) {
        self.state
            .counts
            .source_drops
            .fetch_add(1, Ordering::SeqCst);
    }
}

struct PendingSetup(Arc<State>);

impl Future for PendingSetup {
    type Output = FieldResult<ErasedSubscription>;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

impl Drop for PendingSetup {
    fn drop(&mut self) {
        self.0
            .counts
            .setup_future_drops
            .fetch_add(1, Ordering::SeqCst);
    }
}

struct PendingEvent(Arc<State>);

impl Future for PendingEvent {
    type Output = FieldResult<ResolvedValue>;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

impl Drop for PendingEvent {
    fn drop(&mut self) {
        self.0
            .counts
            .event_future_drops
            .fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Debug)]
pub struct SecretFailure(pub &'static str);

impl std::fmt::Display for SecretFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for SecretFailure {}
