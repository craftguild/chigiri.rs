//! Bounded request-level OpenTelemetry span hierarchy.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use chigiri_runtime::{
    ErrorEvent, LifecycleEvent, LifecyclePhase, LifecycleResult, RequestId, SamplingDecision,
};
use opentelemetry::Context;
use opentelemetry::trace::{Span, Status, TraceContextExt, Tracer};

use super::{error_attributes, lifecycle_attributes};

const DEFAULT_MAX_ACTIVE_REQUESTS: usize = 4096;

pub(super) struct SpanTracker<T> {
    tracer: T,
    active: Arc<Mutex<ActiveSpans>>,
}

impl<T> SpanTracker<T> {
    pub(super) fn new(tracer: T) -> Self {
        Self {
            tracer,
            active: Arc::new(Mutex::new(ActiveSpans {
                requests: HashMap::new(),
                max_requests: DEFAULT_MAX_ACTIVE_REQUESTS,
            })),
        }
    }

    pub(super) const fn tracer(&self) -> &T {
        &self.tracer
    }
}

impl<T: Clone> Clone for SpanTracker<T> {
    fn clone(&self) -> Self {
        Self {
            tracer: self.tracer.clone(),
            active: Arc::clone(&self.active),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for SpanTracker<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SpanTracker")
            .field("tracer", &self.tracer)
            .finish_non_exhaustive()
    }
}

impl<T> SpanTracker<T>
where
    T: Tracer + Send + Sync + 'static,
    T::Span: Send + Sync + 'static,
{
    pub(super) fn observe(&self, event: LifecycleEvent<'_>) {
        let emit_new = !Context::current().is_telemetry_suppressed();
        if event.identity().sampling() == SamplingDecision::Drop {
            return;
        }
        let Some(request_id) = event.identity().request_id() else {
            if emit_new {
                self.short_lifecycle(event, &Context::new());
            }
            return;
        };
        if event.phase() == LifecyclePhase::RequestStarted
            && event.result() == LifecycleResult::Started
        {
            if emit_new {
                self.start_request(request_id, event);
            }
            return;
        }
        if event.phase() == LifecyclePhase::RequestCompleted {
            // Suppression prevents new telemetry, but cannot defer retirement
            // of spans already owned by this observer or consume their slots.
            self.finish_request(request_id, event, emit_new);
            return;
        }
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(request) = active.requests.get_mut(request_id) else {
            drop(active);
            if emit_new {
                self.short_lifecycle(event, &Context::new());
            }
            return;
        };
        request.observe(&self.tracer, event, emit_new);
    }

    pub(super) fn observe_error(&self, event: ErrorEvent<'_>) {
        if Context::current().is_telemetry_suppressed() {
            return;
        }
        let parent = event
            .identity()
            .request_id()
            .and_then(|request_id| {
                let active = self
                    .active
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                active
                    .requests
                    .get(request_id)
                    .map(|request| request.parent_for(event.phase()))
            })
            .unwrap_or_default();
        let mut span = self.tracer.start_with_context("chigiri.error", &parent);
        span.set_attributes(error_attributes(&event));
        span.set_status(Status::error(event.code()));
        span.end();
    }

    fn start_request(&self, request_id: &RequestId, event: LifecycleEvent<'_>) {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if active.requests.contains_key(request_id) || active.requests.len() >= active.max_requests
        {
            return;
        }
        let root = start_span(&self.tracer, "graphql.request", &Context::new(), event);
        active.requests.insert(
            request_id.clone(),
            RequestSpans {
                root,
                operation: None,
                execution: None,
                providers: HashMap::new(),
            },
        );
    }

    fn finish_request(&self, request_id: &RequestId, event: LifecycleEvent<'_>, emit_new: bool) {
        let request = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .requests
            .remove(request_id);
        if let Some(request) = request {
            request.finish(event);
        } else if emit_new {
            self.short_lifecycle(event, &Context::new());
        }
    }

    fn short_lifecycle(&self, event: LifecycleEvent<'_>, parent: &Context) {
        let mut span = self
            .tracer
            .start_with_context(super::span_name(event.phase(), event.component()), parent);
        apply_event(&mut span, event);
        span.end();
    }
}

struct ActiveSpans {
    requests: HashMap<RequestId, RequestSpans>,
    max_requests: usize,
}

struct RequestSpans {
    root: Context,
    operation: Option<Context>,
    execution: Option<Context>,
    providers: HashMap<LifecyclePhase, Context>,
}

impl RequestSpans {
    fn observe<T>(&mut self, tracer: &T, event: LifecycleEvent<'_>, emit_new: bool)
    where
        T: Tracer,
        T::Span: Send + Sync + 'static,
    {
        if event.component() == "field" {
            if emit_new {
                let parent = self.execution.as_ref().unwrap_or(&self.root);
                let mut span = tracer.start_with_context("graphql.field.resolve", parent);
                apply_event(&mut span, event);
                span.end();
            }
            return;
        }
        if event.result() == LifecycleResult::Started {
            if emit_new {
                self.start_phase(tracer, event);
            }
        } else {
            self.finish_phase(event, emit_new);
        }
    }

    fn start_phase<T>(&mut self, tracer: &T, event: LifecycleEvent<'_>)
    where
        T: Tracer,
        T::Span: Send + Sync + 'static,
    {
        if is_operation_phase(event.phase()) && self.operation.is_none() {
            self.operation = Some(start_span(
                tracer,
                "graphql.operation.resolve",
                &self.root,
                event,
            ));
        }
        if event.phase() == LifecyclePhase::Execution && self.execution.is_none() {
            let parent = self.operation.as_ref().unwrap_or(&self.root);
            self.execution = Some(start_span(
                tracer,
                "graphql.operation.execute",
                parent,
                event,
            ));
            return;
        }
        if is_provider_phase(event.phase()) && !self.providers.contains_key(&event.phase()) {
            let parent = self.operation.as_ref().unwrap_or(&self.root);
            self.providers.insert(
                event.phase(),
                start_span(tracer, provider_span_name(event.phase()), parent, event),
            );
            return;
        }
        self.root
            .span()
            .add_event("chigiri.lifecycle", lifecycle_attributes(event));
    }

    fn finish_phase(&mut self, event: LifecycleEvent<'_>, emit_new: bool) {
        if event.phase() == LifecyclePhase::Execution
            && let Some(context) = self.execution.take()
        {
            finish_context(&context, event);
            return;
        }
        if let Some(context) = self.providers.remove(&event.phase()) {
            finish_context(&context, event);
            return;
        }
        if emit_new {
            self.parent_for(event.phase())
                .span()
                .add_event("chigiri.lifecycle", lifecycle_attributes(event));
        }
    }

    fn parent_for(&self, phase: LifecyclePhase) -> Context {
        self.providers
            .get(&phase)
            .or(self.execution.as_ref())
            .or(self.operation.as_ref())
            .unwrap_or(&self.root)
            .clone()
    }

    fn finish(mut self, event: LifecycleEvent<'_>) {
        for (_, context) in self.providers.drain() {
            context.span().end();
        }
        if let Some(context) = self.execution.take() {
            context.span().end();
        }
        if let Some(context) = self.operation.take() {
            context.span().end();
        }
        finish_context(&self.root, event);
    }
}

fn start_span<T>(
    tracer: &T,
    name: &'static str,
    parent: &Context,
    event: LifecycleEvent<'_>,
) -> Context
where
    T: Tracer,
    T::Span: Send + Sync + 'static,
{
    let mut span = tracer.start_with_context(name, parent);
    apply_event(&mut span, event);
    parent.with_span(span)
}

fn finish_context(context: &Context, event: LifecycleEvent<'_>) {
    context.span().set_attributes(lifecycle_attributes(event));
    if is_failure(event.result()) {
        context
            .span()
            .set_status(Status::error(event.code().unwrap_or("CHIGIRI_FAILURE")));
    }
    context.span().end();
}

fn apply_event(span: &mut impl Span, event: LifecycleEvent<'_>) {
    span.set_attributes(lifecycle_attributes(event));
    if is_failure(event.result()) {
        span.set_status(Status::error(event.code().unwrap_or("CHIGIRI_FAILURE")));
    }
}

fn is_failure(result: LifecycleResult) -> bool {
    matches!(
        result,
        LifecycleResult::Rejected
            | LifecycleResult::Failed
            | LifecycleResult::TimedOut
            | LifecycleResult::Cancelled
    )
}

fn is_operation_phase(phase: LifecyclePhase) -> bool {
    matches!(
        phase,
        LifecyclePhase::OperationSourceResolution
            | LifecyclePhase::DocumentPreparation
            | LifecyclePhase::OperationAdmission
            | LifecyclePhase::Execution
    )
}

fn is_provider_phase(phase: LifecyclePhase) -> bool {
    matches!(
        phase,
        LifecyclePhase::TransportAdmission
            | LifecyclePhase::OperationSourceResolution
            | LifecyclePhase::OperationAdmission
    )
}

fn provider_span_name(phase: LifecyclePhase) -> &'static str {
    match phase {
        LifecyclePhase::TransportAdmission => "chigiri.provider.transport-admission",
        LifecyclePhase::OperationSourceResolution => "chigiri.provider.operation-source",
        LifecyclePhase::OperationAdmission => "chigiri.provider.operation-admission",
        _ => "chigiri.provider",
    }
}
