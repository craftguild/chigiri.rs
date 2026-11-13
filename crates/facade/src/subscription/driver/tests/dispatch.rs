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

pub(super) struct Dispatch(pub(super) Arc<State>);

impl ResolverDispatch<AppContext> for Dispatch {
    fn schema_fingerprint(&self) -> ResolverSchemaFingerprint {
        descriptor_fingerprint(&SCHEMA)
    }

    fn register_custom_scalars(&self, registry: &mut CustomScalarRegistry) {
        let state = Arc::clone(&self.0);
        registry.register("Probe", move |_, value| {
            state.counts.scalar_calls.fetch_add(1, Ordering::SeqCst);
            state.wait_in_scalar();
            #[cfg(feature = "subscription-worker")]
            if matches!(&value, Value::String(text) if text == "panic-after-release") {
                panic!("private validation scalar failure");
            }
            Ok(value)
        });
    }

    fn resolve<'a>(
        &'a self,
        context: &'a AppContext,
        parent_type: &'a str,
        field_name: &'a str,
        parent: Option<&'a ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        let state = Arc::clone(&self.0);
        Box::pin(async move {
            match (parent_type, field_name) {
                ("Query" | "Mutation", "ping") => Ok(ResolvedValue::Leaf(Value::from("pong"))),
                ("Query" | "Mutation", "echo") => arguments
                    .into_values()
                    .next()
                    .and_then(|value| value.into_value())
                    .map(ResolvedValue::Leaf)
                    .ok_or_else(|| invariant_error("echo argument is absent")),
                ("Event", "value") => {
                    state.counts.nested_calls.fetch_add(1, Ordering::SeqCst);
                    let event = parent
                        .and_then(|parent| parent.downcast_ref::<EventObject>())
                        .ok_or_else(|| invariant_error("event parent is absent"))?;
                    assert_eq!(event.generation, context.generation);
                    Ok(ResolvedValue::Leaf(Value::Int(i128::from(event.value))))
                }
                _ => Err(invariant_error("unexpected ordinary coordinate")),
            }
        })
    }
}

impl SubscriptionResolverDispatch<AppContext> for Dispatch {
    #[cfg(feature = "subscription-worker")]
    fn prepare_observation(
        &self,
        _: &AppContext,
        _: &str,
        _: &str,
        receipt: chigiri_runtime::__private::SubscriptionObservationReceipt,
        _: usize,
    ) -> FieldResult<Option<Box<dyn chigiri_runtime::__private::HostSubscriptionObservation>>> {
        super::observation::prepare(&self.0, receipt)
    }

    fn subscription_root(&self) -> Option<&'static str> {
        Some("Subscription")
    }

    fn subscribe<'a>(
        &'a self,
        _: FieldContext<'a, AppContext>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ErasedSubscription>> {
        #[cfg(feature = "subscription-worker")]
        super::observation::entered(&self.0);
        self.0.counts.setup_calls.fetch_add(1, Ordering::SeqCst);
        if self.0.setup_mode == SetupMode::Pending {
            return Box::pin(PendingSetup(Arc::clone(&self.0)));
        }
        let state = Arc::clone(&self.0);
        Box::pin(async move {
            if state.setup_mode == SetupMode::OuterError {
                return Err(FieldError::public("source setup denied").code("SOURCE_DENIED"));
            }
            let limit = typed_limit(arguments)?;
            let steps = state.source.lock().unwrap().clone();
            let stream = ScriptedStream {
                state: Arc::clone(&state),
                steps: VecDeque::from(steps),
                pending_observed: false,
            };
            let mapper_state = Arc::clone(&state);
            let mapper = Arc::new(move |_: SourceFailure| {
                mapper_state
                    .counts
                    .source_maps
                    .fetch_add(1, Ordering::SeqCst);
                FieldError::internal(SecretFailure)
            });
            let source = erase_subscription_source(
                SubscriptionSource::new(stream).with_overflow(SubscriptionOverflow::DropOldest),
                Args { limit },
                mapper,
            );
            #[cfg(feature = "subscription-worker")]
            let source = super::observation::attach(&state, source)?;
            Ok(source)
        })
    }

    fn resolve_event<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
        arguments: &'a ErasedSubscriptionArguments,
        event: &'a ErasedSubscriptionEvent,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
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
    match arguments
        .into_values()
        .next()
        .and_then(|value| value.into_value())
    {
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
    pending_observed: bool,
}

impl Stream for ScriptedStream {
    type Item = Result<Event, SourceFailure>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let result = match self.steps.front().copied() {
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
        };
        // Publish the observed phase before waking a fixture waiting for worker progress.
        let first_pending = result.is_pending() && !self.pending_observed;
        self.pending_observed |= first_pending;
        self.state.record_source_poll(first_pending);
        result
    }
}

#[test]
fn pending_probe_counts_each_source_once_across_repolls() {
    let state = State::new(vec![SourceStep::Pending]);
    let mut first = ScriptedStream {
        state: Arc::clone(&state),
        steps: VecDeque::from([SourceStep::Pending]),
        pending_observed: false,
    };
    let mut second = ScriptedStream {
        state: Arc::clone(&state),
        steps: VecDeque::from([SourceStep::Pending]),
        pending_observed: false,
    };
    let mut context = Context::from_waker(std::task::Waker::noop());
    for _ in 0..3 {
        assert!(Pin::new(&mut first).poll_next(&mut context).is_pending());
        assert_eq!(state.source_pending_instances.load(Ordering::SeqCst), 1);
    }
    assert!(Pin::new(&mut second).poll_next(&mut context).is_pending());
    assert_eq!(state.source_pending_instances.load(Ordering::SeqCst), 2);
}

impl Drop for ScriptedStream {
    fn drop(&mut self) {
        self.state
            .counts
            .source_drops
            .fetch_add(1, Ordering::SeqCst);
        #[cfg(feature = "subscription-worker")]
        self.state.wait_in_source_drop();
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
struct SecretFailure;

impl std::fmt::Display for SecretFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("source secret")
    }
}

impl std::error::Error for SecretFailure {}
