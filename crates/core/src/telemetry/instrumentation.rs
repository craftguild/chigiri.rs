//! Execution ownership is independent of public correlation metadata equality.

use std::fmt::Write;

use super::{RequestTelemetry, TraceId};
use crate::instrumentation::{InstrumentationKind, InstrumentationScope, PropagationContext};

#[derive(Clone, Debug)]
pub(super) enum State {
    Owned(InstrumentationScope),
    Retained(PropagationContext),
}

impl RequestTelemetry {
    /// Captures child work inside the already-active owning SDK execution.
    #[doc(hidden)]
    pub fn instrumentation_child(&self, kind: InstrumentationKind) -> Option<InstrumentationScope> {
        self.instrumentation_scope()
            .map(|scope| scope.child(kind, self.sampling))
    }

    /// Returns an SDK-owned live scope, independently of request-ID availability.
    #[doc(hidden)]
    pub fn instrumentation_scope(&self) -> Option<&InstrumentationScope> {
        match &self.instrumentation {
            Some(State::Owned(scope)) => Some(scope),
            _ => None,
        }
    }

    /// Returns only bounded propagation/control values, without live ownership.
    #[doc(hidden)]
    pub fn propagation_context(&self) -> Option<PropagationContext> {
        self.instrumentation.as_ref().map(|state| match state {
            State::Owned(scope) => scope.context(),
            State::Retained(values) => values.clone(),
        })
    }

    /// Associates owned execution and adopts its actual trace ID when valid.
    ///
    /// An opaque legacy ID is never interpreted as a distributed parent.
    #[doc(hidden)]
    pub fn with_instrumentation_scope(mut self, scope: InstrumentationScope) -> Self {
        if let Some(parent) = scope.context().parent() {
            let mut trace = String::with_capacity(32);
            for byte in parent.trace_id() {
                write!(trace, "{byte:02x}").expect("writing to a String cannot fail");
            }
            self.trace_id = Some(TraceId::new(trace).expect("fixed hexadecimal trace identity"));
        }
        self.instrumentation = Some(State::Owned(scope));
        self
    }

    /// Replaces live ownership with correlation that fits a checked byte budget.
    ///
    /// Consumers account their fixed metadata container separately and supply
    /// remaining optional-correlation capacity before retaining state. Even
    /// with zero capacity, the fixed suppression control remains present.
    #[doc(hidden)]
    pub fn retain_instrumentation(mut self, available: usize) -> Self {
        let retained = self
            .propagation_context()
            .map(|values| State::Retained(values.retain_within(available)));
        self.instrumentation = retained;
        self
    }

    /// Budgets optional ID text while preserving short finite execution ownership.
    #[doc(hidden)]
    pub fn retain_instrumentation_trace_id(&mut self, available: usize) {
        if self.instrumentation.is_some()
            && self
                .trace_id
                .as_ref()
                .is_some_and(|id| id.as_str().len() > available)
        {
            self.trace_id = None;
        }
    }

    /// Copies detached instrumentation metadata after checking optional capacity.
    ///
    /// Configured trace-ID text and parent/state share the supplied budget.
    /// Required request identity is accounted separately by the owner. Legacy
    /// metadata without instrumentation retains its existing accounting rules.
    #[doc(hidden)]
    pub fn retained_instrumentation(&self, available: usize) -> Self {
        let Some(values) = self.propagation_context() else {
            return self.clone();
        };
        let trace_id = self
            .trace_id
            .as_ref()
            .filter(|id| id.as_str().len() <= available)
            .cloned();
        let remaining = available - trace_id.as_ref().map_or(0, |id| id.as_str().len());
        Self {
            request_id: self.request_id.clone(),
            trace_id,
            sampling: self.sampling,
            instrumentation: Some(State::Retained(values.retain_within(remaining))),
        }
    }
}

// Equality retains the existing public correlation/sampling meaning. Scope
// handles describe execution ownership and never supply request identity.
impl PartialEq for RequestTelemetry {
    fn eq(&self, other: &Self) -> bool {
        self.request_id == other.request_id
            && self.trace_id == other.trace_id
            && self.sampling == other.sampling
    }
}

impl Eq for RequestTelemetry {}

impl std::fmt::Debug for RequestTelemetry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RequestTelemetry")
            .field("request_id", &self.request_id)
            .field("trace_id", &self.trace_id)
            .field("sampling", &self.sampling)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instrumentation::TraceParent;
    use crate::{RequestId, SamplingDecision};

    #[test]
    fn detached_metadata_budgets_derived_id_and_parent_but_never_suppression() {
        let values = PropagationContext::new(
            Some(TraceParent::new([1; 16], [2; 8], 1, true, "vendor=value").unwrap()),
            true,
        );
        let parent_bytes = values.correlation_bytes();
        let original = RequestTelemetry {
            request_id: Some(RequestId::new("required-request").unwrap()),
            trace_id: Some(TraceId::new("01010101010101010101010101010101").unwrap()),
            sampling: SamplingDecision::Drop,
            instrumentation: Some(State::Retained(values)),
        };
        for capacity in [0, 31, 32, 32 + parent_bytes - 1, 32 + parent_bytes] {
            let retained = original.retained_instrumentation(capacity);
            assert_eq!(retained.request_id(), original.request_id());
            assert_eq!(retained.sampling(), SamplingDecision::Drop);
            assert_eq!(retained.trace_id().is_some(), capacity >= 32);
            assert!(retained.instrumentation_scope().is_none());
            let values = retained.propagation_context().unwrap();
            assert!(values.is_suppressed());
            assert_eq!(values.parent().is_some(), capacity == 32 + parent_bytes);
        }
        assert!(original.propagation_context().unwrap().parent().is_some());
        assert!(original.trace_id().is_some());
        let legacy = RequestTelemetry::new(
            None,
            Some(TraceId::new("legacy").unwrap()),
            SamplingDecision::Record,
        );
        assert_eq!(legacy.retained_instrumentation(0), legacy);
    }
}
