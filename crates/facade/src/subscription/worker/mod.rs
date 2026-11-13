//! Optional concrete worker ownership for official Tokio bridges.

mod active;
mod completion;
mod deadline;
mod error;
mod initial_resources;
mod managed_task;
pub(crate) mod observation;
mod pending;
mod port;
mod process;
mod registration;
mod task_ownership;
mod validation;
#[cfg(test)]
pub(crate) use process::diagnostic_for_test as process_diagnostic_for_test;
#[cfg(test)]
pub(crate) use process::poisoned_for_test as process_poisoned_for_test;
#[cfg(test)]
pub(crate) use process::supervisors::abort_for_test as abort_managed_for_test;
#[cfg(test)]
pub(crate) use process::supervisors::poisoned_for_test as managed_supervisor_poisoned_for_test;
#[cfg(test)]
pub(crate) use process::supervisors::retained_for_test as managed_supervisor_retained_for_test;
#[cfg(test)]
pub(crate) use process::supervisors::retained_socket_tickets_for_test as managed_socket_tickets_for_test;
pub(crate) use process::supervisors::spawn_managed_supervisor;
pub(crate) mod websocket;

#[cfg(test)]
mod test_waker;
#[cfg(test)]
mod tests;

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};

pub(crate) use active::{
    ActiveSetupFailureKind, ActiveSetupOutcome, ActiveSubscriptionWorker, ActiveWorkerSignal,
    InitialResponseDeadline, InitialResponseGate, SourceAndInitialCompletion as InitialTaskOwner,
    run_events,
};
pub(crate) use completion::{
    ConcreteWorker, ConcreteWorkerOutcome, ErasedWorkerCompletion, RetiredWorkerOwner,
    WorkerCompletionOutcome, WorkerRetainedState,
};
pub(crate) use completion::{dispose_payload, dispose_value};
pub(crate) use deadline::{
    DeadlineArmer, DeadlineRole, HttpPhaseDeadline, MonitoredDeadline, MonitoredDeadlinePair,
    MonitoredDeadlineTable, run_monitored,
};
pub use error::SubscriptionWorkerRuntimeError;
pub(crate) use initial_resources::{InitialResponseBufferLease, InitialWorkerResources};
pub(crate) use managed_task::{guard_authorization, guard_instrumented_work};
pub(crate) use pending::{PendingPreparationOutcome, PendingPreparationWorker};
pub use process::SubscriptionProcessShutdown;
pub(crate) use registration::{EndpointWorkerOwner, EndpointWorkerShutdown, WorkerGenerationSlot};
pub(crate) use task_ownership::{TaskOwnedFuture, TaskOwnedValue};

/// Explicit opt-in ownership for one endpoint's concrete streaming workers.
#[doc(hidden)]
pub struct SubscriptionRuntimeGuard {
    registry: Option<crate::subscription::delivery::QuarantineRegistry>,
}

impl SubscriptionRuntimeGuard {
    pub(crate) const fn new(registry: crate::subscription::delivery::QuarantineRegistry) -> Self {
        Self {
            registry: Some(registry),
        }
    }

    /// Stops new workers for this endpoint and waits for its exact slots only.
    pub fn shutdown(
        mut self,
    ) -> Result<SubscriptionRuntimeShutdown, SubscriptionWorkerRuntimeError> {
        let registry = self
            .registry
            .take()
            .expect("live subscription runtime guard");
        let shutdown = registry
            .detach_worker_runtime()
            .map_err(|_| SubscriptionWorkerRuntimeError::Invariant)?;
        Ok(SubscriptionRuntimeShutdown {
            inner: shutdown,
            registry,
        })
    }
}

impl Drop for SubscriptionRuntimeGuard {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.take() {
            drop(registry.detach_worker_runtime());
        }
    }
}

/// Endpoint-isolated drain after its streaming runtime guard is consumed.
#[must_use = "endpoint streaming shutdown must be awaited"]
#[doc(hidden)]
pub struct SubscriptionRuntimeShutdown {
    inner: EndpointWorkerShutdown,
    registry: crate::subscription::delivery::QuarantineRegistry,
}

#[cfg(test)]
impl SubscriptionRuntimeShutdown {
    pub(crate) fn retained_states_for_test(&self) -> [usize; 7] {
        self.inner.retained_states_for_test()
    }

    pub(crate) fn into_registration_shutdown(self) -> EndpointWorkerShutdown {
        let Self { inner, registry } = self;
        drop(registry);
        inner
    }
}

impl Future for SubscriptionRuntimeShutdown {
    type Output = Result<(), SubscriptionWorkerRuntimeError>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        loop {
            match self.registry.poll_one(context) {
                std::task::Poll::Ready(Ok(Some(_))) => {}
                std::task::Poll::Ready(Ok(None)) => {
                    return std::pin::Pin::new(&mut self.inner).poll(context);
                }
                std::task::Poll::Ready(Err(_)) => {
                    return std::task::Poll::Ready(Err(SubscriptionWorkerRuntimeError::Invariant));
                }
                std::task::Poll::Pending => return std::task::Poll::Pending,
            }
        }
    }
}

pub(crate) fn register_endpoint() -> Result<EndpointWorkerOwner, SubscriptionWorkerRuntimeError> {
    process::register_endpoint()
}

pub(crate) fn shutdown_process_runtime()
-> Result<process::SubscriptionProcessShutdown, SubscriptionWorkerRuntimeError> {
    process::shutdown_process_runtime()
}

/// Begins the one-shot final drain of the process subscription reaper.
#[doc(hidden)]
pub fn shutdown_subscription_runtime_process()
-> Result<SubscriptionProcessShutdown, SubscriptionWorkerRuntimeError> {
    shutdown_process_runtime()
}

pub(crate) fn spawn_worker<F>(
    slot: &WorkerGenerationSlot,
    future: F,
) -> Result<ConcreteWorker<F::Output>, SubscriptionWorkerRuntimeError>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let handle = tokio::runtime::Handle::try_current()
        .map_err(|_| SubscriptionWorkerRuntimeError::Startup)?;
    let use_slot = slot.start()?;
    match catch_unwind(AssertUnwindSafe(|| handle.spawn(future))) {
        Ok(task) => Ok(ConcreteWorker::new(task, use_slot)),
        Err(_) => {
            let _ = use_slot.complete();
            Err(SubscriptionWorkerRuntimeError::Shutdown)
        }
    }
}
