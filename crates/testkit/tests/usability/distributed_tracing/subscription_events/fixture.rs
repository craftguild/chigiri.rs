//! Ordinary generated C=() events inspect standard context, never a context span field.

use std::future::poll_fn;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll};

use chigiri_http::{RequestPhase, SubscriptionTransport};
use futures_util::Stream;
use opentelemetry::Context;
use opentelemetry::trace::{SpanContext, TraceContextExt};
use subscription_generated as generated;

use super::super::PrivateValue;
use super::behavior::{self, Mode};

pub(super) struct Seen {
    pub(super) stage: &'static str,
    pub(super) phase: RequestPhase,
    pub(super) span: SpanContext,
    pub(super) suppressed: bool,
    pub(super) private: bool,
    pub(super) trace_id: Option<String>,
}

pub(super) struct Probe {
    pub(super) profile: SubscriptionTransport,
    pub(super) seen: Mutex<Vec<Seen>>,
    pub(super) mode: Mode,
    pub(super) panic_release: Option<tokio::sync::Notify>,
}
impl Probe {
    pub(super) fn new(profile: SubscriptionTransport) -> Self {
        Self {
            profile,
            seen: Mutex::default(),
            mode: Mode::Success,
            panic_release: None,
        }
    }
    pub(super) fn setup(&self) -> RequestPhase {
        RequestPhase::SubscriptionSetup {
            transport: self.profile,
        }
    }
    pub(super) fn event(&self, sequence: u64) -> RequestPhase {
        RequestPhase::SubscriptionEvent {
            transport: self.profile,
            sequence,
        }
    }
    pub(super) fn observe(
        &self,
        stage: &'static str,
        phase: RequestPhase,
        trace_id: Option<String>,
    ) -> SpanContext {
        let current = Context::current();
        let span = current.span().span_context().clone();
        let mut seen = self.seen.lock().unwrap();
        assert!(seen.len() < 64, "bounded event context probe");
        seen.push(Seen {
            stage,
            phase,
            span: span.clone(),
            suppressed: current.is_telemetry_suppressed(),
            private: current.get::<PrivateValue>().is_some(),
            trace_id,
        });
        span
    }
}

async fn pause() {
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
}

pub(super) struct Lease {
    probe: Arc<Probe>,
    phase: RequestPhase,
    stage: &'static str,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.probe.observe(self.stage, self.phase, None);
    }
}
pub(super) struct Factory(pub(super) Arc<Probe>);
impl chigiri_http::ContextFactory<(), Lease> for Factory {
    fn create<'a>(
        &'a self,
        request: chigiri_http::RequestContext<'a>,
    ) -> chigiri::BoxFuture<'a, Result<chigiri::PreparedContext<(), Lease>, chigiri::PrepareError>>
    {
        let phase = request.phase();
        let trace_id = request
            .transport()
            .request_telemetry()
            .unwrap()
            .trace_id()
            .map(|id| id.as_str().to_owned());
        self.0.observe("factory-construct", phase, trace_id.clone());
        let future = Lease {
            probe: Arc::clone(&self.0),
            phase,
            stage: "factory-drop",
        };
        Box::pin(async move {
            self.0.observe("factory-poll", phase, trace_id.clone());
            behavior::prepare(&self.0, phase).await?;
            pause().await;
            self.0.observe("factory-resume", phase, trace_id);
            drop(future);
            Ok(chigiri::PreparedContext::new(
                (),
                Lease {
                    probe: Arc::clone(&self.0),
                    phase,
                    stage: "state-drop",
                },
            ))
        })
    }
}

pub(super) struct Types;
#[derive(Clone)]
pub(super) struct Tick {
    sequence: u64,
    probe: Arc<Probe>,
}
impl Drop for Tick {
    fn drop(&mut self) {
        self.probe
            .observe("event-drop", self.probe.event(self.sequence), None);
    }
}
impl generated::SchemaTypes for Types {
    type Tick = Tick;
}
struct Source {
    probe: Arc<Probe>,
    next: u64,
}
impl Stream for Source {
    type Item = chigiri::FieldResult<Tick>;
    fn poll_next(
        mut self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
    ) -> Poll<Option<Self::Item>> {
        self.probe.observe("source-poll", self.probe.setup(), None);
        self.next += 1;
        match self.next {
            1 => {
                context.waker().wake_by_ref();
                Poll::Pending
            }
            2 | 3 => Poll::Ready(Some(Ok(Tick {
                sequence: self.next - 1,
                probe: Arc::clone(&self.probe),
            }))),
            _ => Poll::Ready(None),
        }
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        self.probe.observe("source-drop", self.probe.setup(), None);
    }
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
        self.0.observe("subscribe", self.0.setup(), None);
        Ok(chigiri::SubscriptionSource::new(Source {
            probe: Arc::clone(&self.0),
            next: 0,
        }))
    }
    async fn resolve_ticks_event<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        event: &'a Tick,
    ) -> chigiri::FieldResult<Tick> {
        self.0
            .observe("resolve", self.0.event(event.sequence), None);
        behavior::resolve(&self.0, event.sequence).await?;
        pause().await;
        self.0
            .observe("resolve-resume", self.0.event(event.sequence), None);
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
        event: &'a Tick,
    ) -> chigiri::FieldResult<i32> {
        Ok(event.sequence.try_into().unwrap())
    }
    async fn padding<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        event: &'a Tick,
    ) -> chigiri::FieldResult<String> {
        let before = self_context(event, "padding");
        pause().await;
        let after = self_context(event, "padding-resume");
        assert_eq!(before, after);
        Ok(format!(
            "{}:{}:{}",
            before.trace_id(),
            before.span_id(),
            Context::current().is_telemetry_suppressed()
        ))
    }
}
fn self_context(event: &Tick, stage: &'static str) -> SpanContext {
    event
        .probe
        .observe(stage, event.probe.event(event.sequence), None)
}

pub(super) fn resolvers(probe: Arc<Probe>) -> generated::Resolvers<(), Types> {
    generated::Resolvers::builder(std::convert::identity::<chigiri::FieldError>)
        .with_query(Query)
        .with_mutation(Mutation)
        .with_subscription(Subscription(probe))
        .with_tick(TickFields)
        .build()
}
