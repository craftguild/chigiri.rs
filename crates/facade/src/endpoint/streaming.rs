//! Additive admitted HTTP ingress for official subscription bridges.

use chigiri_http::{BoxFuture, DeadlineFactory, RequestBody, TransportRequest};

use crate::__streaming::SubscriptionHttpIngress;

use super::Endpoint;

impl<C: Send + Sync + 'static> Endpoint<C> {
    /// Creates one bounded aggregate driver; the application must poll and drain it.
    ///
    /// Explicit worker-runtime registration is separate. No task is spawned.
    #[doc(hidden)]
    #[cfg(feature = "subscription-worker")]
    pub fn __http_streaming_supervision(
        &self,
    ) -> Result<
        (
            crate::__streaming::HttpStreamingRegistration,
            crate::__streaming::HttpStreamingDriver,
        ),
        crate::__streaming::HttpStreamingError,
    > {
        let state = self.neutral.shared_state();
        crate::__streaming::HttpStreamingRegistration::new(
            state.subscriptions.resources.delivery().byte_owner(),
            state
                .subscriptions
                .settings
                .limits()
                .max_active_subscriptions,
        )
    }

    /// Runs the shared HTTP policies once and returns finite or admitted input.
    ///
    /// A pending result owns its original deadlines and quota but starts no
    /// application validation/source task. The official worker must consume it;
    /// dropping it records cancellation and releases the still-pending owner.
    #[doc(hidden)]
    pub fn __handle_http_streaming<'a, 'deadline: 'a, B>(
        &'a self,
        request: TransportRequest<'a>,
        body: B,
        deadlines: &'deadline dyn DeadlineFactory,
    ) -> BoxFuture<'a, SubscriptionHttpIngress<'deadline, C>>
    where
        B: RequestBody + 'a,
    {
        crate::transport::streaming::handle(self.neutral.shared_state(), request, body, deadlines)
    }
}
