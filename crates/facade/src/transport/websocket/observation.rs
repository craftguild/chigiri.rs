//! Initial request lifecycle ownership before an application performs HTTP 101.

use std::time::Instant;

use chigiri_runtime::{ExecutableSchema, LifecyclePhase, LifecycleResult, RequestTelemetry};

use crate::transport::InFlightRequest;
use crate::transport::observation::status_result;

pub(super) struct UpgradeObservation<C: Send + Sync + 'static> {
    schema: ExecutableSchema<C>,
    telemetry: RequestTelemetry,
    started: Instant,
    finished: bool,
    _in_flight: InFlightRequest,
}

impl<C: Send + Sync + 'static> UpgradeObservation<C> {
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
            "websocket-upgrade",
        );
        Self {
            schema,
            telemetry,
            started: Instant::now(),
            finished: false,
            _in_flight: in_flight,
        }
    }

    pub(super) const fn telemetry(&self) -> &RequestTelemetry {
        &self.telemetry
    }

    pub(super) fn finish_status(self, status: u16) {
        let (result, code) = status_result(status);
        self.finish(result, code);
    }

    pub(super) fn finish(mut self, result: LifecycleResult, code: Option<&'static str>) {
        self.finished = true;
        self.schema.observe_lifecycle(
            LifecyclePhase::RequestCompleted,
            result,
            Some(self.started),
            Some(&self.telemetry),
            code,
            "websocket-upgrade",
        );
        if let Some(scope) = self.telemetry.instrumentation_scope() {
            scope.finish(result);
        }
    }
}

impl<C: Send + Sync + 'static> Drop for UpgradeObservation<C> {
    fn drop(&mut self) {
        if !self.finished {
            self.schema.observe_lifecycle(
                LifecyclePhase::RequestCompleted,
                LifecycleResult::Cancelled,
                Some(self.started),
                Some(&self.telemetry),
                None,
                "websocket-upgrade",
            );
            if let Some(scope) = self.telemetry.instrumentation_scope() {
                scope.finish(LifecycleResult::Cancelled);
            }
        }
    }
}
