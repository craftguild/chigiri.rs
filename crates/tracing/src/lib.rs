#![forbid(unsafe_code)]
//! Opt-in bridge from bounded Chigiri events to `tracing`.

use chigiri_runtime::{
    ErrorEvent, ErrorObserver, LifecycleEvent, LifecycleObserver, LifecyclePhase, LifecycleResult,
    SamplingDecision,
};
use tracing::{Level, event, span};

/// `tracing` observer that does not record secret or unbounded identity values.
#[derive(Clone, Copy, Debug, Default)]
pub struct TracingObserver;

impl TracingObserver {
    /// Creates a stateless observer.
    pub const fn new() -> Self {
        Self
    }
}

impl LifecycleObserver for TracingObserver {
    fn observe(&self, event: LifecycleEvent<'_>) {
        if event.identity().sampling() == SamplingDecision::Drop {
            return;
        }
        let identity = event.identity();
        let phase = phase_name(event.phase());
        let result = result_name(event.result());
        let code = event.code().unwrap_or("");
        let duration_ns = event
            .duration()
            .map(|duration| duration.as_nanos().min(u128::from(u64::MAX)) as u64)
            .unwrap_or(0);
        let current = span!(
            target: "chigiri",
            Level::INFO,
            "chigiri.lifecycle",
            chigiri.phase = phase,
            chigiri.result = result,
            chigiri.component = event.component(),
            chigiri.code = code,
            chigiri.duration_ns = duration_ns,
            chigiri.has_request_id = identity.request_id().is_some(),
            chigiri.has_trace_id = identity.trace_id().is_some(),
            chigiri.has_operation = identity.operation().is_some(),
        );
        let _entered = current.enter();
        event!(target: "chigiri", Level::INFO, "chigiri lifecycle event");
    }
}

impl ErrorObserver for TracingObserver {
    fn observe_error(&self, event: ErrorEvent<'_>) {
        let identity = event.identity();
        let current = span!(
            target: "chigiri",
            Level::ERROR,
            "chigiri.error",
            chigiri.phase = phase_name(event.phase()),
            chigiri.component = event.component(),
            chigiri.code = event.code(),
            chigiri.has_error_id = event.error_id().is_some(),
            chigiri.has_request_id = identity.request_id().is_some(),
            chigiri.has_trace_id = identity.trace_id().is_some(),
            chigiri.has_operation = identity.operation().is_some(),
            chigiri.has_path = event.path().is_some(),
            chigiri.has_coordinate = event.coordinate().is_some(),
        );
        let _entered = current.enter();
        event!(target: "chigiri", Level::ERROR, "chigiri internal error");
    }
}

fn phase_name(phase: LifecyclePhase) -> &'static str {
    match phase {
        LifecyclePhase::ServerBuilt => "server-built",
        LifecyclePhase::RequestStarted => "request-started",
        LifecyclePhase::HttpPreflight => "http-preflight",
        LifecyclePhase::TransportAdmission => "transport-admission",
        LifecyclePhase::BodyRead => "body-read",
        LifecyclePhase::HttpDecode => "http-decode",
        LifecyclePhase::OperationSourceResolution => "operation-source-resolution",
        LifecyclePhase::DecodedRequestHook => "decoded-request-hook",
        LifecyclePhase::ContextConstruction => "context-construction",
        LifecyclePhase::RequestPlugin => "request-plugin",
        LifecyclePhase::DocumentPreparation => "document-preparation",
        LifecyclePhase::OperationAdmission => "operation-admission",
        LifecyclePhase::Execution => "execution",
        LifecyclePhase::ResponseHook => "response-hook",
        LifecyclePhase::HttpEncode => "http-encode",
        LifecyclePhase::RequestCompleted => "request-completed",
    }
}

fn result_name(result: LifecycleResult) -> &'static str {
    match result {
        LifecycleResult::Started => "started",
        LifecycleResult::Succeeded => "succeeded",
        LifecycleResult::Rejected => "rejected",
        LifecycleResult::Failed => "failed",
        LifecycleResult::TimedOut => "timed-out",
        LifecycleResult::Cancelled => "cancelled",
        LifecycleResult::CacheHit => "cache-hit",
        LifecycleResult::CacheMiss => "cache-miss",
    }
}

#[cfg(test)]
mod tests {
    use super::{TracingObserver, phase_name, result_name};
    use chigiri_runtime::{
        ErrorObserver, LifecycleEvent, LifecycleObserver, LifecyclePhase, LifecycleResult,
        RequestIdentity, SamplingDecision,
    };

    #[test]
    fn bridge_implements_both_observer_contracts() {
        fn accepts(_: &dyn LifecycleObserver, _: &dyn ErrorObserver) {}
        let observer = TracingObserver::new();
        accepts(&observer, &observer);
    }

    #[test]
    fn span_fields_use_only_closed_phase_and_result_values() {
        assert_eq!(phase_name(LifecyclePhase::Execution), "execution");
        assert_eq!(result_name(LifecycleResult::TimedOut), "timed-out");
    }

    #[test]
    fn dropped_sampling_lifecycle_events_are_ignored_safely() {
        TracingObserver::new().observe(LifecycleEvent::new(
            LifecyclePhase::Execution,
            LifecycleResult::Started,
            None,
            RequestIdentity::new(None, None, None).with_sampling(SamplingDecision::Drop),
            None,
            "test",
        ));
    }
}
