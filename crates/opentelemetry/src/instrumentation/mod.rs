//! Owned, bounded execution scopes without a request-ID span registry.

mod capacity;
mod propagation;
mod scope;

#[cfg(test)]
mod tests;

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use chigiri_runtime::SamplingDecision;
use chigiri_runtime::instrumentation::{
    Instrumentation, InstrumentationKind, InstrumentationScope, PropagationCarrier,
    PropagationContext,
};
use opentelemetry::trace::Tracer;

// A closed optional-instrumentation bound, independent of SDK admission limits.
const MAX_LIVE_SPANS: usize = 4096;

/// Execution instrumentation using an application-owned OpenTelemetry tracer.
///
/// Register once with `ObservabilityComponents::instrumentation`. This provider
/// extracts bounded W3C headers, owns SDK spans, and activates sanitized context
/// for SDK work. It never installs global telemetry or shuts down the tracer.
/// Application processors must not block on exporter I/O or queue capacity.
///
/// Legacy `OpenTelemetryObserver` registration is independent. Registering both
/// built-in paths deliberately records overlapping SDK lifecycle spans.
///
/// The facade registration is
/// `components.observability().instrumentation(OpenTelemetryInstrumentation::new(tracer))`.
/// Generated resolvers and context factories then use ordinary
/// `opentelemetry::Context::current()` without an application context span field.
/// Current valid middleware parentage wins over bounded W3C carrier fallback.
/// Direct execution has no implicit carrier. Arbitrary Context values and baggage
/// are excluded from SDK retention.
///
/// Standard telemetry suppression is separate from lifecycle sampling and remains
/// explicit even without a parent or retained optional correlation. Suppressed
/// SDK work creates no backend spans while independent error observers still run.
/// The provider owns at most 4096 simultaneous SDK spans; declined/capacity-bound
/// work keeps its selected propagation/control. Export storage is not guaranteed.
///
/// Subscription setup ends before waiting. Each admitted recorded event gets a
/// fresh root and creation-time setup link; suppressed events still activate the
/// immutable setup control. Publisher and previous-event context are excluded.
/// Applications explicitly propagate standard Context to their own spawned tasks
/// and downstream clients. A compatible tracing-opentelemetry layer with context
/// activation enabled connects ordinary contextual application spans.
pub struct OpenTelemetryInstrumentation<T> {
    shared: Arc<Shared<T>>,
}

struct Shared<T> {
    tracer: T,
    active: Arc<AtomicUsize>,
    limit: usize,
    #[cfg(test)]
    retained_contexts: Arc<AtomicUsize>,
}

impl<T> OpenTelemetryInstrumentation<T> {
    /// Uses the injected tracer without taking ownership of exporter shutdown.
    pub fn new(tracer: T) -> Self {
        Self {
            shared: Arc::new(Shared {
                tracer,
                active: Arc::new(AtomicUsize::new(0)),
                limit: MAX_LIVE_SPANS,
                #[cfg(test)]
                retained_contexts: Arc::new(AtomicUsize::new(0)),
            }),
        }
    }

    /// Returns the application-selected tracer.
    pub fn tracer(&self) -> &T {
        &self.shared.tracer
    }
}

impl<T> Clone for OpenTelemetryInstrumentation<T> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<T> fmt::Debug for OpenTelemetryInstrumentation<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenTelemetryInstrumentation")
            .finish_non_exhaustive()
    }
}

impl<T> Instrumentation for OpenTelemetryInstrumentation<T>
where
    T: Tracer + Send + Sync + 'static,
    T::Span: Send + Sync + 'static,
{
    fn capture(&self, carrier: &dyn PropagationCarrier) -> PropagationContext {
        propagation::capture(carrier)
    }

    fn start(
        &self,
        context: &PropagationContext,
        kind: InstrumentationKind,
        sampling: SamplingDecision,
    ) -> InstrumentationScope {
        scope::start(Arc::clone(&self.shared), context, kind, sampling, true)
    }
}
