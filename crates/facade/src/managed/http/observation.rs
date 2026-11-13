//! Closed task-side fault reporting; no application observer lives in recovery.

use chigiri_runtime::{ExecutableSchema, LifecyclePhase};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone)]
pub(crate) struct HttpFaultReporter(Arc<dyn ObserveFault>);

trait ObserveFault: Send + Sync {
    fn report(&self);
}

struct SchemaFault<C: Send + Sync + 'static> {
    schema: ExecutableSchema<C>,
    component: &'static str,
    reported: AtomicBool,
}

impl<C: Send + Sync + 'static> ObserveFault for SchemaFault<C> {
    fn report(&self) {
        if self.reported.swap(true, Ordering::AcqRel) {
            return;
        }
        self.schema.observe_internal_error(
            LifecyclePhase::Execution,
            "INTERNAL_SERVER_ERROR",
            &HttpTaskFailure,
            None,
            self.component,
        );
    }
}

impl HttpFaultReporter {
    pub(crate) fn authorization(
        observer: Arc<dyn crate::transport::websocket::AuthorizationErrorObserver>,
    ) -> Self {
        Self(Arc::new(AuthorizationFault(observer)))
    }

    pub(crate) fn new<C: Send + Sync + 'static>(schema: ExecutableSchema<C>) -> Self {
        Self::with_component(schema, "managed-http-execution")
    }

    pub(crate) fn subscription<C: Send + Sync + 'static>(schema: ExecutableSchema<C>) -> Self {
        Self::with_component(schema, "managed-subscription-execution")
    }

    fn with_component<C: Send + Sync + 'static>(
        schema: ExecutableSchema<C>,
        component: &'static str,
    ) -> Self {
        Self(Arc::new(SchemaFault {
            schema,
            component,
            reported: AtomicBool::new(false),
        }))
    }

    pub(crate) fn report(&self) {
        self.0.report();
    }
}

#[derive(Debug)]
struct HttpTaskFailure;
impl std::fmt::Display for HttpTaskFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Managed HTTP execution failed")
    }
}
impl std::error::Error for HttpTaskFailure {}

struct AuthorizationFault(Arc<dyn crate::transport::websocket::AuthorizationErrorObserver>);
impl ObserveFault for AuthorizationFault {
    fn report(&self) {
        self.0.observe(&AuthorizationTaskFailure);
    }
}

#[derive(Debug)]
struct AuthorizationTaskFailure;
impl std::fmt::Display for AuthorizationTaskFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WebSocket connection task failed internally")
    }
}
impl std::error::Error for AuthorizationTaskFailure {}
