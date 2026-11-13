//! Credential-free internal error observation for initialization failures.

use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use chigiri_runtime::{ExecutableSchema, LifecyclePhase, RequestTelemetry};

use super::authorization::AuthorizationErrorObserver;

struct Observer<C: Send + Sync + 'static> {
    schema: ExecutableSchema<C>,
    telemetry: RequestTelemetry,
    reported: AtomicBool,
    phase: LifecyclePhase,
    component: &'static str,
}

impl<C: Send + Sync + 'static> AuthorizationErrorObserver for Observer<C> {
    fn observe(&self, source: &(dyn Error + Send + Sync + 'static)) {
        if self.reported.swap(true, Ordering::AcqRel) {
            return;
        }
        self.schema.observe_internal_error(
            self.phase,
            "INTERNAL_SERVER_ERROR",
            source,
            Some(&self.telemetry),
            self.component,
        );
    }
}

pub(crate) fn new<C: Send + Sync + 'static>(
    schema: ExecutableSchema<C>,
    telemetry: RequestTelemetry,
) -> Arc<dyn AuthorizationErrorObserver> {
    Arc::new(Observer {
        schema,
        telemetry,
        reported: AtomicBool::new(false),
        phase: LifecyclePhase::TransportAdmission,
        component: "websocket-connection-init",
    })
}

pub(crate) fn native<C: Send + Sync + 'static>(
    schema: ExecutableSchema<C>,
    telemetry: RequestTelemetry,
) -> Arc<dyn AuthorizationErrorObserver> {
    Arc::new(Observer {
        schema,
        telemetry,
        reported: AtomicBool::new(false),
        phase: LifecyclePhase::Execution,
        component: "managed-websocket-session",
    })
}
