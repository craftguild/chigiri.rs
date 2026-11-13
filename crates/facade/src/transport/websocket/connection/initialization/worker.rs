//! Spawn only the closed authorization role with its original join integrity.

use std::sync::Arc;

use super::super::authorization::{AuthorizationFrameworkFailure, AuthorizationResult, authorize};
use super::super::close::WebSocketConnectionClose;
use super::WebSocketConnectionInitialization;
use crate::BoxFuture;
use crate::managed::HttpFaultReporter;
use chigiri_runtime::instrumentation::{InstrumentationKind, instrument_future};

impl WebSocketConnectionInitialization {
    pub(super) fn start_authorization(&mut self) -> Result<(), WebSocketConnectionClose> {
        if self.activity.is_stopping() {
            return Err(self.terminate(WebSocketConnectionClose::INTERNAL));
        }
        let instrumentation = self.request.instrumentation().map(|captured| {
            captured.handoff_scope(InstrumentationKind::Provider {
                phase: chigiri_runtime::LifecyclePhase::TransportAdmission,
                component: "websocket-authorization",
            })
        });
        let work = authorize(
            Arc::clone(&self.request),
            self.payload.clone(),
            Arc::clone(&self.authorizer),
            Arc::clone(&self.observer),
            self.activity.clone(),
        );
        let work: BoxFuture<'static, AuthorizationResult> = if self.activity.is_managed() {
            crate::subscription::worker::guard_authorization(
                work,
                self.activity.clone(),
                HttpFaultReporter::authorization(Arc::clone(&self.observer)),
                instrumentation,
            )
        } else {
            match instrumentation {
                Some(scope) => instrument_future(Some(scope), work),
                None => work,
            }
        };
        let mut worker = self
            .connection
            .as_ref()
            .expect("live initialization connection")
            .spawn_worker(work)
            .map_err(|_| self.fail_framework(AuthorizationFrameworkFailure::Startup))?;
        if self.activity.is_managed() {
            worker.retain_subscription_integrity(self.activity.clone());
        }
        self.worker = Some(worker);
        Ok(())
    }
}
