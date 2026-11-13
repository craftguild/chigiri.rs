//! An immutable entry snapshot selected before any SDK task handoff.

use std::fmt;
use std::sync::Arc;

use chigiri_core::{RequestTelemetry, SamplingDecision, TraceId};

use super::{
    Instrumentation, InstrumentationKind, InstrumentationScope, PropagationCarrier,
    PropagationContext,
};

/// Owns the selected provider and bounded parent/control snapshot, without a span.
#[doc(hidden)]
#[derive(Clone)]
pub struct CapturedInstrumentation {
    provider: Arc<dyn Instrumentation>,
    values: PropagationContext,
}

impl CapturedInstrumentation {
    pub(crate) fn new(
        provider: Arc<dyn Instrumentation>,
        carrier: &dyn PropagationCarrier,
    ) -> Self {
        let values = provider.capture(carrier);
        Self { provider, values }
    }

    pub(crate) fn from_values(
        provider: Arc<dyn Instrumentation>,
        values: PropagationContext,
    ) -> Self {
        Self { provider, values }
    }

    /// Borrows sanitized correlation/control without any live span ownership.
    pub fn values(&self) -> &PropagationContext {
        &self.values
    }

    /// Retains optional correlation only within an already checked byte budget.
    /// The fixed provider/control capability never owns a live execution span.
    pub fn retain_within(&self, available: usize) -> Self {
        Self {
            provider: Arc::clone(&self.provider),
            values: self.values.retain_within(available),
        }
    }

    /// Activates captured control for lazy work, including unpolled destruction.
    ///
    /// This does not create an SDK lifecycle span before work starts.
    pub fn handoff_scope(&self, kind: InstrumentationKind) -> InstrumentationScope {
        self.provider
            .start(&self.values, kind, SamplingDecision::Drop)
    }

    /// Selects a child handoff inside an already-active owning SDK execution.
    /// Nested standard control is captured before work moves to another poller.
    pub fn child_handoff_scope(&self, kind: InstrumentationKind) -> InstrumentationScope {
        let values = self.provider.capture(&());
        self.provider.start(&values, kind, SamplingDecision::Drop)
    }

    pub(crate) fn correlation(&self) -> Option<TraceId> {
        self.values.parent().map(|parent| {
            TraceId::new(format!("{:032x}", u128::from_be_bytes(parent.trace_id())))
                .expect("fixed hexadecimal trace identity")
        })
    }

    pub(crate) fn metadata<R>(&self, kind: InstrumentationKind, callback: impl FnOnce() -> R) -> R {
        // Identity factories and sampling run under owned control before the
        // lifecycle decision permits creating an SDK request/event span.
        let scope = self
            .provider
            .start(&self.values, kind, SamplingDecision::Drop);
        scope.run(callback)
    }

    /// Binds an owned scope without repeating identity creation or sampling.
    pub fn start(
        &self,
        telemetry: RequestTelemetry,
        kind: InstrumentationKind,
    ) -> RequestTelemetry {
        let scope = self
            .provider
            .start(&self.values, kind, telemetry.sampling());
        telemetry.with_instrumentation_scope(scope)
    }
}

impl fmt::Debug for CapturedInstrumentation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CapturedInstrumentation")
            .field("values", &self.values)
            .finish_non_exhaustive()
    }
}
