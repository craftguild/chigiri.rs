//! Endpoint-issued opaque quotas used by the official streaming adapters.

use crate::__streaming::{
    BridgeCleanupDriver, BridgeConnection, BridgeResourceError, HttpHeartbeatStaging,
    PendingBridgeOperation, SubscriptionControlSignal, SubscriptionOutbox,
    SubscriptionOutboxSignal, WebSocketControlStaging,
};
use crate::{SubscriptionOverflow, SubscriptionTransport};

use super::Endpoint;

impl<C: Send + Sync + 'static> Endpoint<C> {
    #[cfg(all(test, feature = "subscription-worker"))]
    pub(crate) fn subscription_connection_counts_for_test(&self) -> (usize, usize) {
        let state = self.neutral.shared_state();
        let resources = state.subscriptions.resources.delivery();
        (
            resources.snapshot().endpoint_connections,
            resources.byte_owner().used(),
        )
    }

    #[cfg(all(test, feature = "subscription-worker"))]
    pub(crate) fn subscription_pending_probe_for_test(
        &self,
    ) -> impl Fn() -> (usize, usize) + Send + Sync + 'static {
        let resources = self
            .neutral
            .shared_state()
            .subscriptions
            .resources
            .delivery()
            .clone();
        move || {
            (
                resources.snapshot().endpoint_pending,
                resources.byte_owner().used(),
            )
        }
    }

    #[cfg(all(test, feature = "subscription-worker"))]
    pub(crate) fn subscription_physical_counts_for_test(&self) -> (usize, usize) {
        let state = self.neutral.shared_state();
        let resources = state.subscriptions.resources.delivery();
        (
            resources.snapshot().endpoint_active_subscriptions,
            resources.byte_owner().used(),
        )
    }

    /// Creates a fixed control FIFO charged to this endpoint's shared byte owner.
    #[doc(hidden)]
    pub fn __websocket_control_staging(&self) -> WebSocketControlStaging {
        WebSocketControlStaging::new(
            self.neutral
                .shared_state()
                .subscriptions
                .resources
                .delivery()
                .byte_owner(),
        )
    }

    /// Creates one coalesced heartbeat marker for an HTTP subscription stream.
    #[doc(hidden)]
    pub fn __http_heartbeat_staging(
        &self,
        transport: SubscriptionTransport,
    ) -> Result<HttpHeartbeatStaging, SubscriptionControlSignal> {
        HttpHeartbeatStaging::new(
            self.neutral
                .shared_state()
                .subscriptions
                .resources
                .delivery()
                .byte_owner(),
            transport,
        )
    }

    /// Creates the common outbox using this endpoint's validated shared graph.
    ///
    /// The adapter must pass the source's server-selected overflow policy and
    /// retain its admitted operation charge through physical source cleanup.
    #[doc(hidden)]
    pub fn __subscription_outbox(
        &self,
        overflow: SubscriptionOverflow,
        transport: SubscriptionTransport,
        operation_id: Option<std::sync::Arc<str>>,
    ) -> Result<SubscriptionOutbox, SubscriptionOutboxSignal> {
        let state = self.neutral.shared_state();
        SubscriptionOutbox::new(
            &state.subscriptions.settings,
            state.subscriptions.resources.delivery().byte_owner(),
            overflow,
            transport,
            operation_id,
        )
    }

    /// Reserves endpoint pending capacity before an HTTP subscription setup.
    ///
    /// The adapter must transfer this token to active ownership before source
    /// initialization and retain the active token through physical cleanup.
    #[doc(hidden)]
    pub fn __begin_http_subscription(&self) -> Result<PendingBridgeOperation, BridgeResourceError> {
        self.neutral
            .shared_state()
            .subscriptions
            .resources
            .begin_http_subscription()
    }

    /// Opens an endpoint-charged multiplexed protocol connection.
    ///
    /// This neither performs transport admission nor upgrades the HTTP request.
    #[doc(hidden)]
    pub fn __open_subscription_connection(&self) -> Result<BridgeConnection, BridgeResourceError> {
        self.neutral
            .shared_state()
            .subscriptions
            .resources
            .open_connection()
    }

    /// Returns poll-only access to detached, physically charged cleanup work.
    #[doc(hidden)]
    pub fn __subscription_cleanup_driver(&self) -> BridgeCleanupDriver {
        self.neutral
            .shared_state()
            .subscriptions
            .resources
            .cleanup_driver()
    }
}
