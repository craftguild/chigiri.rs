//! Endpoint-shared activation; service ownership is separate from request work.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, Weak};

use tokio::runtime::{Handle, Id};

use super::requests::RequestRegistry;
use super::sockets::SocketRegistry;
use super::{Control, ManagedHttpRequest, ManagedServiceError, Supervisor};
use crate::__streaming::SubscriptionRuntimeGuard;
use crate::subscription::worker::spawn_managed_supervisor;
use crate::transport::streaming::supervisor::{HttpStreamingDriver, HttpStreamingRegistration};

pub(crate) struct Binding {
    pub(crate) control: Arc<Control>,
    state: Mutex<BindingState>,
}

struct BindingState {
    runtime: Option<Id>,
    service: Weak<ServiceLife>,
}

impl Binding {
    pub(crate) fn new() -> Self {
        Self {
            control: Control::new(),
            state: Mutex::new(BindingState {
                runtime: None,
                service: Weak::new(),
            }),
        }
    }

    pub(crate) fn activate(
        &self,
        websocket_capacity: usize,
        create: impl FnOnce() -> Result<
            (
                HttpStreamingRegistration,
                HttpStreamingDriver,
                SubscriptionRuntimeGuard,
            ),
            ManagedServiceError,
        >,
    ) -> Result<ManagedService, ManagedServiceError> {
        let runtime = Handle::try_current().map_err(|_| ManagedServiceError::MissingRuntime)?;
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                drop(poisoned.into_inner());
                self.control.fail();
                return Err(ManagedServiceError::Infrastructure);
            }
        };
        if state.runtime.is_some_and(|id| id != runtime.id()) {
            return Err(ManagedServiceError::RuntimeMismatch);
        }
        if self.control.is_stopping() {
            return Err(ManagedServiceError::Closed);
        }
        if let Some(life) = state.service.upgrade() {
            return Ok(ManagedService { life });
        }
        // Tokio has no fallible timer-enabled query. Contain the documented
        // missing-timer panic at construction, before any service can escape.
        drop(
            catch_unwind(AssertUnwindSafe(|| {
                tokio::time::sleep(std::time::Duration::ZERO)
            }))
            .map_err(|_| ManagedServiceError::Startup)?,
        );
        self.control.activation_started()?;
        let (registration, driver, guard) = create().inspect_err(|_| self.control.fail())?;
        let (sockets, socket_driver) = SocketRegistry::new(
            Arc::clone(&self.control),
            runtime.clone(),
            websocket_capacity,
        )
        .inspect_err(|_| self.control.fail())?;
        let (requests, request_driver) =
            RequestRegistry::new(Arc::clone(&self.control)).inspect_err(|_| self.control.fail())?;
        let life = Arc::new(ServiceLife {
            control: Arc::clone(&self.control),
            registration: registration.clone(),
            runtime: runtime.id(),
            sockets: Arc::clone(&sockets),
            requests,
        });
        spawn_managed_supervisor(Supervisor::new(
            Arc::clone(&self.control),
            registration,
            driver,
            socket_driver,
            request_driver,
            guard,
        ))?;
        state.runtime = Some(runtime.id());
        state.service = Arc::downgrade(&life);
        Ok(ManagedService { life })
    }
}

#[cfg(test)]
mod tests;

struct ServiceLife {
    control: Arc<Control>,
    registration: HttpStreamingRegistration,
    runtime: Id,
    sockets: Arc<SocketRegistry>,
    requests: Arc<RequestRegistry>,
}

impl Drop for ServiceLife {
    fn drop(&mut self) {
        self.control.stop();
    }
}

/// Internal managed-service capability issued only by the endpoint facade.
///
/// Clones share one supervisor. Request work never retains this service-owner
/// lease, so dropping the last routed service requests cancellation promptly.
#[derive(Clone)]
#[doc(hidden)]
pub struct ManagedService {
    life: Arc<ServiceLife>,
}

impl ManagedService {
    /// Acquires one ordered SDK request scope without starting application work.
    pub fn request(&self) -> Result<ManagedHttpRequest, ManagedServiceError> {
        let runtime = Handle::try_current().map_err(|_| ManagedServiceError::MissingRuntime)?;
        if runtime.id() != self.life.runtime {
            return Err(ManagedServiceError::RuntimeMismatch);
        }
        let permit = self.life.control.admit()?;
        Ok(ManagedHttpRequest::new(
            permit,
            Arc::clone(&self.life.control),
            self.life.registration.clone(),
            Arc::clone(&self.life.sockets),
            Arc::clone(&self.life.requests),
        ))
    }
}
