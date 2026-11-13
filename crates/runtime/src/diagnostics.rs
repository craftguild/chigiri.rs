//! Observer configuration shared by core plugins and HTTP adapters.

mod field;
mod instrumentation;

pub(crate) use field::FieldDiagnosticsPlugin;
mod internal;

use std::error::Error;
use std::sync::Arc;
use std::time::Instant;

use crate::instrumentation::Instrumentation;
use chigiri_core::{
    ErrorEvent, ErrorId, ErrorIdFactory, ErrorObserver, LifecycleEvent, LifecycleObserver,
    LifecyclePhase, LifecycleResult, OperationIdentity, RequestIdFactory, RequestIdentity,
    RequestTelemetry, Sampler, SamplingDecision, TraceId,
};

#[derive(Clone, Default)]
pub(crate) struct Diagnostics {
    error_ids: Option<Arc<dyn ErrorIdFactory>>,
    lifecycle: Option<Arc<dyn LifecycleObserver>>,
    errors: Option<Arc<dyn ErrorObserver>>,
    request_ids: Option<Arc<dyn RequestIdFactory>>,
    sampler: Option<Arc<dyn Sampler>>,
    field_instrumentation: bool,
    instrumentation: Option<Arc<dyn Instrumentation>>,
}

impl Diagnostics {
    pub(crate) fn with_instrumentation(mut self, provider: impl Instrumentation) -> Self {
        self.instrumentation = Some(Arc::new(provider));
        self
    }

    pub(crate) fn instrumentation(&self) -> Option<&dyn Instrumentation> {
        self.instrumentation.as_deref()
    }

    pub(crate) fn with_error_ids(mut self, factory: impl ErrorIdFactory) -> Self {
        self.error_ids = Some(Arc::new(factory));
        self
    }
    pub(crate) fn with_lifecycle(mut self, observer: impl LifecycleObserver) -> Self {
        self.lifecycle = Some(Arc::new(observer));
        self
    }
    pub(crate) fn with_errors(mut self, observer: impl ErrorObserver) -> Self {
        self.errors = Some(Arc::new(observer));
        self
    }
    pub(crate) fn with_request_ids(mut self, factory: impl RequestIdFactory) -> Self {
        self.request_ids = Some(Arc::new(factory));
        self
    }
    pub(crate) fn with_sampler(mut self, sampler: impl Sampler) -> Self {
        self.sampler = Some(Arc::new(sampler));
        self
    }
    pub(crate) fn with_field_instrumentation(mut self, enabled: bool) -> Self {
        self.field_instrumentation = enabled;
        self
    }
    pub(crate) const fn field_instrumentation(&self) -> bool {
        self.field_instrumentation
    }
    pub(crate) fn lifecycle(&self) -> Option<&dyn LifecycleObserver> {
        self.lifecycle.as_deref()
    }
    pub(crate) fn observe(
        &self,
        phase: LifecyclePhase,
        result: LifecycleResult,
        started: Option<Instant>,
        identity: RequestIdentity<'_>,
        code: Option<&'static str>,
        component: &'static str,
    ) {
        let Some(observer) = &self.lifecycle else {
            return;
        };
        observer.observe(LifecycleEvent::new(
            phase,
            result,
            started.map(|started| started.elapsed()),
            identity,
            code,
            component,
        ));
    }
    pub(crate) fn observe_error(
        &self,
        phase: LifecyclePhase,
        code: &'static str,
        source: &(dyn Error + Send + Sync + 'static),
        operation: Option<&OperationIdentity>,
        telemetry: Option<&RequestTelemetry>,
        component: &'static str,
    ) -> Option<ErrorId> {
        let error_id = self.create_error_id(phase, operation, telemetry);
        self.notify_error(
            ErrorEvent::new(
                error_id.as_ref(),
                telemetry.map_or_else(
                    || RequestIdentity::new(None, None, operation),
                    |telemetry| telemetry.identity(operation),
                ),
                phase,
                code,
                source,
                None,
                None,
                component,
            ),
            telemetry,
        );
        error_id
    }

    fn create_error_id(
        &self,
        phase: LifecyclePhase,
        operation: Option<&OperationIdentity>,
        telemetry: Option<&RequestTelemetry>,
    ) -> Option<ErrorId> {
        let factory = self.error_ids.as_ref()?;
        match factory.create() {
            Ok(error_id) => Some(error_id),
            Err(failure) => {
                self.notify_error(
                    ErrorEvent::new(
                        None,
                        telemetry.map_or_else(
                            || RequestIdentity::new(None, None, operation),
                            |telemetry| telemetry.identity(operation),
                        ),
                        phase,
                        "ERROR_ID_GENERATION_FAILED",
                        &failure,
                        None,
                        None,
                        "error-id-factory",
                    ),
                    telemetry,
                );
                None
            }
        }
    }

    pub(crate) fn start_phase<'a>(
        &'a self,
        phase: LifecyclePhase,
        component: &'static str,
        telemetry: Option<&'a RequestTelemetry>,
    ) -> DiagnosticPhase<'a> {
        DiagnosticPhase::start(self, phase, phase, component, telemetry)
    }

    pub(crate) fn start_request<'a>(
        &'a self,
        component: &'static str,
        telemetry: Option<&'a RequestTelemetry>,
    ) -> DiagnosticPhase<'a> {
        DiagnosticPhase::start(
            self,
            LifecyclePhase::RequestStarted,
            LifecyclePhase::RequestCompleted,
            component,
            telemetry,
        )
    }

    pub(crate) fn request_telemetry(&self, trace_id: Option<TraceId>) -> RequestTelemetry {
        let (request_id, factory_failure) = match &self.request_ids {
            Some(factory) => match factory.create() {
                Ok(request_id) => (Some(request_id), None),
                Err(failure) => (None, Some(failure)),
            },
            None => (None, None),
        };
        let sampling = self
            .sampler
            .as_ref()
            .map_or(SamplingDecision::Record, |sampler| {
                sampler.sample(RequestIdentity::new(
                    request_id.as_ref(),
                    trace_id.as_ref(),
                    None,
                ))
            });
        let telemetry = RequestTelemetry::new(request_id, trace_id, sampling);
        if let Some(failure) = factory_failure {
            self.notify_error(
                ErrorEvent::new(
                    None,
                    telemetry.identity(None),
                    LifecyclePhase::RequestStarted,
                    "REQUEST_ID_GENERATION_FAILED",
                    &failure,
                    None,
                    None,
                    "request-id-factory",
                ),
                Some(&telemetry),
            );
        }
        telemetry
    }
}

pub(crate) struct DiagnosticPhase<'a> {
    diagnostics: &'a Diagnostics,
    completed_phase: LifecyclePhase,
    component: &'static str,
    started: Instant,
    completed: bool,
    telemetry: Option<&'a RequestTelemetry>,
}

impl<'a> DiagnosticPhase<'a> {
    fn start(
        diagnostics: &'a Diagnostics,
        started_phase: LifecyclePhase,
        completed_phase: LifecyclePhase,
        component: &'static str,
        telemetry: Option<&'a RequestTelemetry>,
    ) -> Self {
        diagnostics.observe(
            started_phase,
            LifecycleResult::Started,
            None,
            request_identity(None, telemetry),
            None,
            component,
        );
        Self {
            diagnostics,
            completed_phase,
            component,
            started: Instant::now(),
            completed: false,
            telemetry,
        }
    }

    pub(crate) fn finish(
        mut self,
        result: LifecycleResult,
        operation: Option<&OperationIdentity>,
        code: Option<&'static str>,
    ) {
        self.diagnostics.observe(
            self.completed_phase,
            result,
            Some(self.started),
            request_identity(operation, self.telemetry),
            code,
            self.component,
        );
        self.completed = true;
    }
}

impl Drop for DiagnosticPhase<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.diagnostics.observe(
                self.completed_phase,
                LifecycleResult::Cancelled,
                Some(self.started),
                request_identity(None, self.telemetry),
                None,
                self.component,
            );
        }
    }
}

pub(crate) fn request_identity<'a>(
    operation: Option<&'a OperationIdentity>,
    telemetry: Option<&'a RequestTelemetry>,
) -> RequestIdentity<'a> {
    telemetry.map_or_else(
        || RequestIdentity::new(None, None, operation),
        |telemetry| telemetry.identity(operation),
    )
}
