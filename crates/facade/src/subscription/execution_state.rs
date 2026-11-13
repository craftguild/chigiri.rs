//! Resource-free execution graph retained by a source or detached cleanup work.
//! Its optional weak managed link cannot retain lifecycle or recovery owners.

use std::sync::Arc;

use chigiri_runtime::ExecutableSchema;

use super::settings::ValidatedSubscriptionSettings;
use crate::transport::{ErasedContextFactory, ErasedResponseHook, TransportSettings};

/// This graph deliberately has no endpoint, writer, quota, or quarantine handle.
pub(crate) struct SubscriptionExecutionState<C> {
    #[cfg(feature = "subscription-worker")]
    pub(crate) managed: std::sync::OnceLock<std::sync::Weak<crate::managed::Control>>,
    pub(crate) schema: ExecutableSchema<C>,
    pub(crate) context_factory: Arc<dyn ErasedContextFactory<C>>,
    pub(crate) response_hook: Option<Arc<dyn ErasedResponseHook>>,
    pub(crate) settings: TransportSettings,
    pub(crate) subscriptions: ValidatedSubscriptionSettings,
}

#[cfg(feature = "subscription-worker")]
impl<C: Send + Sync + 'static> SubscriptionExecutionState<C> {
    pub(crate) fn task_scope(
        &self,
    ) -> (
        crate::managed::EventActivity,
        Option<crate::managed::HttpFaultReporter>,
    ) {
        let activity = crate::managed::EventActivity::new(self.managed.get().cloned());
        let reporter = activity
            .is_managed()
            .then(|| crate::managed::HttpFaultReporter::subscription(self.schema.clone()));
        (activity, reporter)
    }
}

#[cfg(feature = "subscription-worker")]
impl<C> SubscriptionExecutionState<C> {
    pub(crate) fn setup_stopped_error(
        &self,
    ) -> crate::subscription::worker::SubscriptionWorkerRuntimeError {
        use crate::subscription::worker::SubscriptionWorkerRuntimeError;
        match self.managed.get().and_then(std::sync::Weak::upgrade) {
            Some(control)
                if control.outcome() != Some(crate::ShutdownOutcome::InfrastructureFailure) =>
            {
                SubscriptionWorkerRuntimeError::EndpointClosed
            }
            _ => SubscriptionWorkerRuntimeError::Invariant,
        }
    }

    /// Authorizes one setup/admission boundary without retaining a lock across
    /// application code, reservation disposal, task creation or wake delivery.
    pub(crate) fn authorize_start(&self) -> bool {
        let Some(managed) = self.managed.get() else {
            return true;
        };
        let Some(control) = managed.upgrade() else {
            return false;
        };
        control.registration_gate().is_ok()
    }

    /// Pending subscription setup is not eligible active event work. Select
    /// endpoint stop before polling it again and destroy it in its SDK task.
    pub(crate) async fn pending_phase<F: std::future::Future>(
        &self,
        future: F,
    ) -> Option<F::Output> {
        let Some(managed) = self.managed.get() else {
            return Some(future.await);
        };
        let control = managed.upgrade()?;
        tokio::select! {
            biased;
            _ = control.source_stopped() => None,
            result = future => Some(result),
        }
    }
}
