//! Request observation that outlives ingress but not initial response preparation.

use std::time::Instant;

use chigiri_runtime::{ExecutableSchema, LifecyclePhase, LifecycleResult, RequestTelemetry};

use crate::transport::InFlightRequest;
use crate::transport::observation::status_result;

pub(crate) struct InitialRequestObservation<C: Send + Sync + 'static> {
    schema: ExecutableSchema<C>,
    telemetry: RequestTelemetry,
    started: Instant,
    finished: bool,
    _in_flight: InFlightRequest,
}

impl<C: Send + Sync + 'static> InitialRequestObservation<C> {
    pub(super) fn new(
        schema: ExecutableSchema<C>,
        telemetry: RequestTelemetry,
        in_flight: InFlightRequest,
    ) -> Self {
        schema.observe_lifecycle(
            LifecyclePhase::RequestStarted,
            LifecycleResult::Started,
            None,
            Some(&telemetry),
            None,
            "http-endpoint",
        );
        Self {
            schema,
            telemetry,
            started: Instant::now(),
            finished: false,
            _in_flight: in_flight,
        }
    }

    pub(crate) fn finish_status(self, status: u16) {
        let (result, code) = status_result(status);
        self.finish(result, code);
    }

    pub(super) const fn telemetry(&self) -> &RequestTelemetry {
        &self.telemetry
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn stream_failure_reporter(
        &self,
        telemetry: Option<&RequestTelemetry>,
    ) -> super::supervisor::StreamFailureReporter {
        let mut reporter = super::supervisor::StreamFailureReporter::new(
            self.schema.clone(),
            self.telemetry.clone(),
        );
        reporter.bind_retained_telemetry(telemetry);
        reporter
    }

    #[cfg(feature = "subscription-worker")]
    pub(super) fn worker_failure(
        &self,
        media: chigiri_http::ResponseMediaType,
        error: &(dyn std::error::Error + Send + Sync + 'static),
    ) -> chigiri_http::ResponseParts {
        let header_bytes = 16 + "content-type".len() + media.content_type().len();
        crate::transport::response::bounded_internal_error_response(
            &self.schema,
            media,
            crate::subscription::control::MAX_INITIAL_RESPONSE_BYTES - header_bytes,
            error,
            LifecyclePhase::TransportAdmission,
            "subscription-initial-response",
            &self.telemetry,
        )
    }

    pub(crate) fn finish(mut self, result: LifecycleResult, code: Option<&'static str>) {
        self.finished = true;
        self.schema.observe_lifecycle(
            LifecyclePhase::RequestCompleted,
            result,
            Some(self.started),
            Some(&self.telemetry),
            code,
            "http-endpoint",
        );
        if let Some(scope) = self.telemetry.instrumentation_scope() {
            scope.finish(result);
        }
    }
}

impl<C: Send + Sync + 'static> Drop for InitialRequestObservation<C> {
    fn drop(&mut self) {
        if !self.finished {
            self.schema.observe_lifecycle(
                LifecyclePhase::RequestCompleted,
                LifecycleResult::Cancelled,
                Some(self.started),
                Some(&self.telemetry),
                None,
                "http-endpoint",
            );
            if let Some(scope) = self.telemetry.instrumentation_scope() {
                scope.finish(LifecycleResult::Cancelled);
            }
        }
    }
}
