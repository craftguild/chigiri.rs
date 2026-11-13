//! Drop guard that notifies the shared observer of facade endpoint lifecycle phases.

use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use chigiri_http::DeadlineFactory;
use chigiri_runtime::instrumentation::{InstrumentationKind, InstrumentationScope};
use chigiri_runtime::{ExecutableSchema, LifecyclePhase, LifecycleResult, RequestTelemetry};

use super::TimeoutKind;
use super::deadline::HttpDeadlineMonitor;

pub(crate) struct Phase<'a, C>
where
    C: Send + Sync + 'static,
{
    schema: &'a ExecutableSchema<C>,
    completed_phase: LifecyclePhase,
    started: Instant,
    completed: bool,
    telemetry: &'a RequestTelemetry,
    instrumentation: Option<InstrumentationScope>,
}

impl<'a, C> Phase<'a, C>
where
    C: Send + Sync + 'static,
{
    pub(crate) fn start(
        schema: &'a ExecutableSchema<C>,
        phase: LifecyclePhase,
        telemetry: &'a RequestTelemetry,
    ) -> Self {
        let instrumentation = telemetry.instrumentation_child(InstrumentationKind::Provider {
            phase,
            component: "http-endpoint",
        });
        schema.observe_lifecycle(
            phase,
            LifecycleResult::Started,
            None,
            Some(telemetry),
            None,
            "http-endpoint",
        );
        Self {
            schema,
            completed_phase: phase,
            started: Instant::now(),
            completed: false,
            telemetry,
            instrumentation,
        }
    }

    pub(crate) fn request(
        schema: &'a ExecutableSchema<C>,
        telemetry: &'a RequestTelemetry,
    ) -> Self {
        schema.observe_lifecycle(
            LifecyclePhase::RequestStarted,
            LifecycleResult::Started,
            None,
            Some(telemetry),
            None,
            "http-endpoint",
        );
        Self {
            schema,
            completed_phase: LifecyclePhase::RequestCompleted,
            started: Instant::now(),
            completed: false,
            telemetry,
            instrumentation: None,
        }
    }

    pub(crate) fn run<R>(&self, callback: impl FnOnce() -> R) -> R {
        match &self.instrumentation {
            Some(scope) => scope.run(callback),
            None => callback(),
        }
    }

    pub(crate) async fn monitor<F, T>(
        &self,
        monitor: &HttpDeadlineMonitor,
        factory: &dyn DeadlineFactory,
        timeout: Duration,
        overall: Pin<&mut (dyn Future<Output = ()> + Send)>,
        create: impl FnOnce() -> F + Send,
    ) -> Result<T, TimeoutKind>
    where
        F: Future<Output = T> + Send,
    {
        let future = monitor.run(self.completed_phase, factory, timeout, overall, create);
        self.instrument(future).await
    }

    pub(crate) async fn instrument<F: Future>(&self, future: F) -> F::Output {
        match &self.instrumentation {
            Some(scope) => scope.instrument(future).await,
            None => future.await,
        }
    }

    pub(crate) fn finish(mut self, result: LifecycleResult, code: Option<&'static str>) {
        self.schema.observe_lifecycle(
            self.completed_phase,
            result,
            Some(self.started),
            Some(self.telemetry),
            code,
            "http-endpoint",
        );
        self.completed = true;
        if let Some(scope) = &self.instrumentation {
            scope.finish(result);
        }
    }
}

impl<C> Drop for Phase<'_, C>
where
    C: Send + Sync + 'static,
{
    fn drop(&mut self) {
        if !self.completed {
            self.schema.observe_lifecycle(
                self.completed_phase,
                LifecycleResult::Cancelled,
                Some(self.started),
                Some(self.telemetry),
                None,
                "http-endpoint",
            );
            if let Some(scope) = &self.instrumentation {
                scope.finish(LifecycleResult::Cancelled);
            }
        }
    }
}

pub(crate) fn status_result(status: u16) -> (LifecycleResult, Option<&'static str>) {
    match status {
        408 | 504 => (LifecycleResult::TimedOut, Some("REQUEST_TIMEOUT")),
        400..=499 => (LifecycleResult::Rejected, None),
        500..=599 => (LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR")),
        _ => (LifecycleResult::Succeeded, None),
    }
}
