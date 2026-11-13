//! Ordinary generated dispatch returns a source that observes actual poll/drop context.

use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll};

use futures_util::Stream;
use opentelemetry::Context;
use opentelemetry::trace::{SpanContext, TraceContextExt};
use subscription_generated as generated;

use super::super::PrivateValue;

pub(super) struct Probe {
    pub(super) parent: SpanContext,
    pub(super) suppressed: bool,
    pub(super) polls: AtomicUsize,
    pub(super) drops: AtomicUsize,
    pub(super) event_drops: AtomicUsize,
    pub(super) fail: bool,
    pub(super) errors: Mutex<Vec<Seen>>,
    pub(super) error_drops: Mutex<Vec<Seen>>,
}

impl Probe {
    pub(super) fn new(parent: SpanContext, suppressed: bool, fail: bool) -> Self {
        Self {
            parent,
            suppressed,
            fail,
            polls: AtomicUsize::new(0),
            drops: AtomicUsize::new(0),
            event_drops: AtomicUsize::new(0),
            errors: Mutex::default(),
            error_drops: Mutex::default(),
        }
    }

    pub(super) fn expected(&self) -> Seen {
        Seen {
            span: self.parent.clone(),
            suppressed: self.suppressed,
            private: false,
        }
    }

    fn assert_current(&self) {
        let current = Context::current();
        assert_eq!(*current.span().span_context(), self.parent);
        assert_eq!(current.is_telemetry_suppressed(), self.suppressed);
        assert!(current.get::<PrivateValue>().is_none());
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct Seen {
    span: SpanContext,
    suppressed: bool,
    private: bool,
}
impl Seen {
    fn current() -> Self {
        let current = Context::current();
        Self {
            span: current.span().span_context().clone(),
            suppressed: current.is_telemetry_suppressed(),
            private: current.get::<PrivateValue>().is_some(),
        }
    }
}

#[derive(Clone)]
pub(super) struct Errors(pub(super) Arc<Probe>);
impl chigiri::ErrorObserver for Errors {
    fn observe_error(&self, _: chigiri::ErrorEvent<'_>) {
        let seen = Seen::current();
        let mut values = self.0.errors.lock().unwrap();
        assert!(
            values.is_empty(),
            "one independent terminal error notification"
        );
        values.push(seen);
    }
}

struct SourceError(Arc<Probe>);
impl std::fmt::Debug for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SourceError")
    }
}
impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("private subscription-source detail")
    }
}
impl std::error::Error for SourceError {}
impl Drop for SourceError {
    fn drop(&mut self) {
        let seen = Seen::current();
        let mut values = self.0.error_drops.lock().unwrap();
        assert!(values.is_empty(), "one terminal error-source destruction");
        values.push(seen);
    }
}

struct Source(Arc<Probe>);

impl Stream for Source {
    type Item = chigiri::FieldResult<Tick>;

    fn poll_next(self: Pin<&mut Self>, task: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        self.0.assert_current();
        if self.0.polls.fetch_add(1, Ordering::SeqCst) == 0 {
            task.waker().wake_by_ref();
            Poll::Pending
        } else if self.0.fail {
            Poll::Ready(Some(Err(chigiri::FieldError::internal(SourceError(
                Arc::clone(&self.0),
            )))))
        } else {
            Poll::Ready(Some(Ok(Tick(Arc::clone(&self.0)))))
        }
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        self.0.assert_current();
        self.0.drops.fetch_add(1, Ordering::SeqCst);
    }
}

pub(super) struct Types;
#[derive(Clone)]
pub(super) struct Tick(Arc<Probe>);
impl Drop for Tick {
    fn drop(&mut self) {
        self.0.assert_current();
        self.0.event_drops.fetch_add(1, Ordering::SeqCst);
    }
}
impl generated::SchemaTypes for Types {
    type Tick = Tick;
}

struct Query;
impl generated::QueryResolver<(), Types, chigiri::FieldError> for Query {
    async fn greeting<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
    ) -> chigiri::FieldResult<String> {
        Ok("hello".into())
    }
}

struct Mutation;
impl generated::MutationResolver<(), Types, chigiri::FieldError> for Mutation {
    async fn acknowledge<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
    ) -> chigiri::FieldResult<String> {
        Ok("accepted".into())
    }
}

struct Subscription(Arc<Probe>);
impl generated::SubscriptionResolver<(), Types, chigiri::FieldError> for Subscription {
    type TicksEvent = Tick;
    type FailingTicksEvent = Tick;

    async fn subscribe_ticks<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
    ) -> chigiri::FieldResult<chigiri::SubscriptionSource<Tick, chigiri::FieldError>> {
        Ok(chigiri::SubscriptionSource::new(Source(Arc::clone(
            &self.0,
        ))))
    }

    async fn resolve_ticks_event<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        event: &'a Tick,
    ) -> chigiri::FieldResult<Tick> {
        Ok(event.clone())
    }

    async fn subscribe_failing_ticks<'a>(
        &'a self,
        context: chigiri::FieldContext<'a, ()>,
    ) -> chigiri::FieldResult<chigiri::SubscriptionSource<Tick, chigiri::FieldError>> {
        self.subscribe_ticks(context).await
    }

    async fn resolve_failing_ticks_event<'a>(
        &'a self,
        context: chigiri::FieldContext<'a, ()>,
        event: &'a Tick,
    ) -> chigiri::FieldResult<Tick> {
        self.resolve_ticks_event(context, event).await
    }
}

struct TickFields;
impl generated::TickResolver<(), Types, chigiri::FieldError> for TickFields {
    async fn value<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        _: &'a Tick,
    ) -> chigiri::FieldResult<i32> {
        Ok(1)
    }

    async fn padding<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        _: &'a Tick,
    ) -> chigiri::FieldResult<String> {
        Ok(String::new())
    }
}

pub(super) fn builder(probe: Arc<Probe>) -> chigiri::spi::ExecutableSchemaBuilder<()> {
    let resolvers = generated::Resolvers::builder(std::convert::identity::<chigiri::FieldError>)
        .with_query(Query)
        .with_mutation(Mutation)
        .with_subscription(Subscription(probe))
        .with_tick(TickFields)
        .build();
    chigiri::spi::ExecutableSchema::builder_with_subscriptions(
        &generated::SCHEMA_DESCRIPTOR,
        resolvers,
    )
}
