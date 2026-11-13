//! Explicit runtime registration, separate from finite endpoint construction.

use super::Endpoint;
use crate::__streaming::{BridgeResourceError, SubscriptionRuntimeGuard};

impl<C: Send + Sync + 'static> Endpoint<C> {
    /// Registers the existing endpoint completion table for official workers.
    ///
    /// This is an explicit streaming opt-in, shared by endpoint clones. Finite
    /// endpoint construction and handlers never start the process reaper. The
    /// application must close its streaming drivers, await this guard's endpoint
    /// drain, and finally drain process supervision before destroying its Tokio
    /// runtimes. This method neither constructs a runtime nor accepts a task.
    #[doc(hidden)]
    pub fn __register_subscription_runtime(
        &self,
    ) -> Result<SubscriptionRuntimeGuard, BridgeResourceError> {
        self.neutral
            .shared_state()
            .subscriptions
            .resources
            .register_worker_runtime()
    }
}
