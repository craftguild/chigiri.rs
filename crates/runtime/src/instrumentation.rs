//! Backend-neutral capture contract shared by SDK entries and HTTP adapters.

mod captured;

use std::sync::Arc;

#[doc(hidden)]
pub use captured::CapturedInstrumentation;

use chigiri_core::SamplingDecision;
pub use chigiri_core::instrumentation::{
    ExecutionScope, InstrumentationGuard, InstrumentationKind, InstrumentationScope,
    InstrumentedFuture, PropagationContext, TraceParent, instrument_future,
};

/// The only incoming carrier fields available to SDK tracing instrumentation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropagationField {
    /// W3C parent context; multiple field values are invalid.
    TraceParent,
    /// W3C vendor state, combined within the fixed carrier bounds.
    TraceState,
}

/// Borrowed carrier access without exposing credentials or application metadata.
///
/// Providers bound reads before copying and use a released standard propagator
/// for syntax. Each visitor borrow ends before the next callback. The carrier
/// is not retained by an owned execution scope.
pub trait PropagationCarrier {
    /// Visits all values of one closed propagation field in transport order.
    fn values(&self, field: PropagationField, visitor: &mut dyn FnMut(&[u8]));
}

impl PropagationCarrier for () {
    fn values(&self, _: PropagationField, _: &mut dyn FnMut(&[u8])) {}
}

/// Optional read-only instrumentation configured once in the component graph.
///
/// Capture is synchronous and precedes SDK task handoff and request observation.
/// Implementations retain bounded parent/control values, not a complete backend
/// Context or application state. Exporters and their shutdown remain app-owned.
pub trait Instrumentation: Send + Sync + 'static {
    /// Captures owning ambient context/suppression, then bounded carrier fallback.
    fn capture(&self, carrier: &dyn PropagationCarrier) -> PropagationContext;

    /// Creates an owned scope with the request's fixed lifecycle sampling.
    ///
    /// Suppression and sampling remain independent. An unrecorded scope still
    /// activates its selected parent and explicit suppressed/unsuppressed state.
    /// Subscription events use fresh roots rather than inheriting setup parentage.
    fn start(
        &self,
        context: &PropagationContext,
        kind: InstrumentationKind,
        sampling: SamplingDecision,
    ) -> InstrumentationScope;
}

impl<T: Instrumentation + ?Sized> Instrumentation for Arc<T> {
    fn capture(&self, carrier: &dyn PropagationCarrier) -> PropagationContext {
        (**self).capture(carrier)
    }

    fn start(
        &self,
        context: &PropagationContext,
        kind: InstrumentationKind,
        sampling: SamplingDecision,
    ) -> InstrumentationScope {
        (**self).start(context, kind, sampling)
    }
}
