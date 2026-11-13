//! Span lifetime and activation belong to the actual SDK-owned execution scope.

mod stored;

use std::sync::Arc;

use chigiri_runtime::instrumentation::{
    ExecutionScope, InstrumentationGuard, InstrumentationKind, InstrumentationScope,
    PropagationContext,
};
use chigiri_runtime::{ErrorEvent, LifecycleResult, SamplingDecision};
use opentelemetry::ContextGuard;
use opentelemetry::trace::{Link, Span, SpanKind, Status, TraceContextExt, Tracer};

use super::Shared;
use super::capacity::Permit;
use super::propagation;
use stored::StoredContext;

struct Scope<T> {
    shared: Arc<Shared<T>>,
    context: Option<StoredContext>,
    values: PropagationContext,
    allow_children: bool,
    _permit: Option<Permit>,
}

struct Guard {
    _context: ContextGuard,
}

impl InstrumentationGuard for Guard {}

pub(super) fn start<T>(
    shared: Arc<Shared<T>>,
    values: &PropagationContext,
    kind: InstrumentationKind,
    sampling: SamplingDecision,
    allow_children: bool,
) -> InstrumentationScope
where
    T: Tracer + Send + Sync + 'static,
    T::Span: Send + Sync + 'static,
{
    let event = kind == InstrumentationKind::SubscriptionEvent;
    let mut selected = if event {
        values.without_parent()
    } else {
        values.clone()
    };
    let mut permit =
        (allow_children && sampling == SamplingDecision::Record && !values.is_suppressed())
            .then(|| Permit::acquire(&shared.active, shared.limit))
            .flatten();
    let context = if permit.is_some() {
        let parent = propagation::context(&selected);
        let mut builder = shared
            .tracer
            .span_builder(name(kind))
            .with_kind(match kind {
                InstrumentationKind::Request | InstrumentationKind::SubscriptionSetup => {
                    SpanKind::Server
                }
                InstrumentationKind::SubscriptionEvent => SpanKind::Consumer,
                _ => SpanKind::Internal,
            });
        if let InstrumentationKind::Provider { phase, component } = kind {
            builder = builder.with_attributes([
                crate::string("chigiri.phase", crate::phase_name(phase)),
                crate::string("chigiri.component", component),
            ]);
        }
        if event && let Some(setup) = values.parent() {
            builder =
                builder.with_links(vec![Link::with_context(propagation::span_context(setup))]);
        }
        let span = shared.tracer.build_with_context(builder, &parent);
        if span.span_context().is_valid() {
            Some(parent.with_span(span))
        } else {
            // A declined span must not replace an adopted parent or turn a
            // deliberately fresh event into a child of setup/worker context.
            drop(span);
            drop(permit.take());
            None
        }
    } else {
        None
    };
    if let Some(context) = &context {
        selected = propagation::snapshot(context.span().span_context(), values.is_suppressed());
    } else {
        selected = propagation::normalize(&selected);
    }
    let allow_children = context.is_some();
    let context = context.map(|context| StoredContext::new(context, &shared));
    InstrumentationScope::new(Scope {
        shared,
        context,
        values: selected,
        allow_children,
        _permit: permit,
    })
}

impl<T> ExecutionScope for Scope<T>
where
    T: Tracer + Send + Sync + 'static,
    T::Span: Send + Sync + 'static,
{
    fn enter(&self) -> Box<dyn InstrumentationGuard + '_> {
        Box::new(Guard {
            _context: self
                .context
                .as_ref()
                .map_or_else(
                    || propagation::context(&self.values),
                    |context| (**context).clone(),
                )
                .attach(),
        })
    }

    fn context(&self) -> PropagationContext {
        self.values.clone()
    }

    fn child(&self, kind: InstrumentationKind, sampling: SamplingDecision) -> InstrumentationScope {
        let selected = propagation::capture(&());
        start(
            Arc::clone(&self.shared),
            &selected,
            kind,
            sampling,
            self.allow_children,
        )
    }

    fn observe_error(&self, event: &ErrorEvent<'_>) {
        let selected = propagation::capture(&());
        if selected.is_suppressed() || selected.parent().is_none() {
            return;
        }
        let Some(_permit) = Permit::acquire(&self.shared.active, self.shared.limit) else {
            return;
        };
        let mut span = self
            .shared
            .tracer
            .start_with_context("chigiri.error", &propagation::context(&selected));
        span.set_attributes(crate::error_attributes(event));
        span.set_status(Status::error(event.code()));
        span.end();
    }

    fn finish(&self, result: LifecycleResult) {
        if let Some(context) = &self.context {
            let span = context.span();
            span.set_attribute(crate::string("chigiri.result", crate::result_name(result)));
            if matches!(result, LifecycleResult::Failed | LifecycleResult::TimedOut) {
                span.set_status(Status::error(crate::result_name(result)));
            }
            span.end();
        }
    }
}

fn name(kind: InstrumentationKind) -> &'static str {
    match kind {
        InstrumentationKind::Request => "graphql.request",
        InstrumentationKind::SubscriptionSetup => "graphql.subscription.setup",
        InstrumentationKind::SubscriptionEvent => "graphql.subscription.event",
        InstrumentationKind::OperationResolution => "graphql.operation.resolve",
        InstrumentationKind::Execution => "graphql.operation.execute",
        InstrumentationKind::Provider { .. } => "chigiri.provider",
        InstrumentationKind::Field => "graphql.field.resolve",
    }
}
