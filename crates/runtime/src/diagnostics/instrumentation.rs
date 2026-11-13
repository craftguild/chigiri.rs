//! Shared capture and metadata preparation for direct and native HTTP entries.

use std::sync::Arc;

use chigiri_core::{ErrorEvent, RequestTelemetry, TraceId};

use super::Diagnostics;
use crate::instrumentation::{
    CapturedInstrumentation, InstrumentationKind, PropagationCarrier, PropagationContext,
};

impl Diagnostics {
    pub(super) fn notify_error(&self, event: ErrorEvent<'_>, telemetry: Option<&RequestTelemetry>) {
        // Callers are inside the owning execution. Reinstalling its initial
        // context would erase application-owned nested suppression.
        if let Some(scope) = telemetry.and_then(RequestTelemetry::instrumentation_scope) {
            scope.observe_error(&event);
        }
        if let Some(observer) = &self.errors {
            observer.observe_error(event);
        }
    }

    pub(crate) fn capture_instrumentation(
        &self,
        carrier: &dyn PropagationCarrier,
    ) -> Option<CapturedInstrumentation> {
        self.instrumentation
            .as_ref()
            .map(|provider| CapturedInstrumentation::new(Arc::clone(provider), carrier))
    }

    pub(crate) fn request_telemetry_with_capture(
        &self,
        trace_id: Option<TraceId>,
        captured: Option<&CapturedInstrumentation>,
        kind: InstrumentationKind,
    ) -> RequestTelemetry {
        match captured {
            Some(captured) => captured.metadata(kind, || {
                let correlation = if kind == InstrumentationKind::SubscriptionEvent {
                    None
                } else {
                    captured.correlation().or(trace_id)
                };
                let telemetry = self.request_telemetry(correlation);
                captured.start(telemetry, kind)
            }),
            None => self.request_telemetry(trace_id),
        }
    }

    pub(crate) fn instrumentation_with_values(
        &self,
        values: PropagationContext,
    ) -> Option<CapturedInstrumentation> {
        self.instrumentation
            .as_ref()
            .map(|provider| CapturedInstrumentation::from_values(Arc::clone(provider), values))
    }
}
