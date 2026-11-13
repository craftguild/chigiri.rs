//! An ordinary generated idle subscription with context and cleanup probes.

use std::future::{pending, poll_fn};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll};

use futures_util::Stream;
use opentelemetry::Context;
use opentelemetry::trace::{SpanContext, TraceContextExt};
use subscription_generated as generated;

use super::super::PrivateValue;

#[derive(Debug)]
pub(super) struct Seen {
    pub(super) stage: &'static str,
    pub(super) span: SpanContext,
    pub(super) suppressed: bool,
    pub(super) private: bool,
}

#[derive(Default)]
pub(super) struct Probe {
    pub(super) seen: Mutex<Vec<Seen>>,
    pub(super) polled: AtomicUsize,
    pub(super) dropped: AtomicUsize,
}
impl Probe {
    fn observe(&self, stage: &'static str) {
        let current = Context::current();
        let mut seen = self.seen.lock().unwrap();
        assert!(seen.len() < 32, "bounded setup probe");
        seen.push(Seen {
            stage,
            span: current.span().span_context().clone(),
            suppressed: current.is_telemetry_suppressed(),
            private: current.get::<PrivateValue>().is_some(),
        });
    }
}

pub(super) struct Lease {
    probe: Arc<Probe>,
    stage: &'static str,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.probe.observe(self.stage);
    }
}

pub(super) struct Factory(pub(super) Arc<Probe>);

struct Completion(Arc<Probe>);
impl chigiri::ResponseCompletion for Completion {
    fn complete(
        &self,
        response: chigiri::ResponseParts,
        _: chigiri::ResponseCompletionOutcome,
    ) -> chigiri::ResponseParts {
        self.0.observe("completion");
        response
    }
}
impl Drop for Completion {
    fn drop(&mut self) {
        self.0.observe("completion-drop");
    }
}
impl chigiri_http::ContextFactory<(), Lease> for Factory {
    fn create<'a>(
        &'a self,
        request: chigiri_http::RequestContext<'a>,
    ) -> chigiri::BoxFuture<'a, Result<chigiri::PreparedContext<(), Lease>, chigiri::PrepareError>>
    {
        assert!(matches!(
            request.phase(),
            chigiri_http::RequestPhase::SubscriptionSetup { .. }
        ));
        self.0.observe("factory-construct");
        let future = Lease {
            probe: Arc::clone(&self.0),
            stage: "factory-drop",
        };
        Box::pin(async move {
            self.0.observe("factory-poll");
            let mut first = true;
            poll_fn(|context| {
                if first {
                    first = false;
                    context.waker().wake_by_ref();
                    Poll::Pending
                } else {
                    Poll::Ready(())
                }
            })
            .await;
            self.0.observe("factory-resume");
            drop(future);
            Ok(chigiri::PreparedContext::new(
                (),
                Lease {
                    probe: Arc::clone(&self.0),
                    stage: "state-drop",
                },
            )
            .with_response_completion(Completion(Arc::clone(&self.0))))
        })
    }
}

pub(super) struct Hook(pub(super) Arc<Probe>);
impl chigiri::ResponseHook<Lease> for Hook {
    fn call<'a>(
        &'a self,
        _: chigiri::ResponseHookContext<'a, Lease>,
    ) -> chigiri::BoxFuture<'a, Result<chigiri::ResponseHeaders, chigiri::HookError>> {
        self.0.observe("hook-construct");
        let future = Lease {
            probe: Arc::clone(&self.0),
            stage: "hook-drop",
        };
        Box::pin(async move {
            self.0.observe("hook-poll");
            tokio::task::yield_now().await;
            self.0.observe("hook-resume");
            drop(future);
            Ok(chigiri::ResponseHeaders::new())
        })
    }
}

struct Source(Arc<Probe>);
impl Stream for Source {
    type Item = chigiri::FieldResult<Tick>;
    fn poll_next(self: Pin<&mut Self>, _: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        if self.0.polled.load(Ordering::SeqCst) == 0 {
            self.0.observe("source-poll");
        }
        // Publish readiness only after the waiting test can see the observation.
        self.0.polled.fetch_add(1, Ordering::SeqCst);
        Poll::Pending
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        self.0.observe("source-drop");
        self.0.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

pub(super) struct Types;
#[derive(Clone)]
pub(super) struct Tick;
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
        self.0.observe("subscribe");
        Ok(chigiri::SubscriptionSource::new(Source(Arc::clone(
            &self.0,
        ))))
    }
    async fn resolve_ticks_event<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        _: &'a Tick,
    ) -> chigiri::FieldResult<Tick> {
        pending().await
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

pub(super) fn resolvers(probe: Arc<Probe>) -> generated::Resolvers<(), Types> {
    generated::Resolvers::builder(std::convert::identity::<chigiri::FieldError>)
        .with_query(Query)
        .with_mutation(Mutation)
        .with_subscription(Subscription(probe))
        .with_tick(TickFields)
        .build()
}
