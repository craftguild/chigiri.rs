//! Explicit pre-upgrade admission for official WebSocket adapters.

use chigiri_http::{BoxFuture, DeadlineFactory, TransportRequest, WebSocketHttpVersion};

use crate::__streaming::{
    AdmittedWebSocketUpgrade, ClaimedWebSocketUpgrade, WebSocketSeedError, WebSocketUpgradeIngress,
};

#[cfg(feature = "subscription-worker")]
use crate::transport::websocket::Handoff;

use super::Endpoint;

impl<C: Send + Sync + 'static> Endpoint<C> {
    /// Drives one upgraded socket through initialization, operations and cleanup.
    ///
    /// The application owns and polls this future inside its registered worker
    /// runtime. Construction starts initialization only after the actual HTTP
    /// upgrade; it never creates a connection task or a runtime.
    #[cfg(feature = "subscription-worker")]
    #[doc(hidden)]
    pub fn __websocket_session<S: crate::__streaming::WebSocketIo>(
        &self,
        seed: ClaimedWebSocketUpgrade<'static>,
        socket: S,
        deadlines: &'static dyn DeadlineFactory,
    ) -> Result<crate::__streaming::WebSocketSessionDriver<C, S>, WebSocketSeedError> {
        self.neutral
            .shared_state()
            .websocket_session(seed, socket, deadlines)
    }

    /// Drives the actual upgraded socket through initialization and ACK flush.
    ///
    /// The application must poll this future under its explicitly registered
    /// worker runtime. Success moves the same socket to the operation session.
    #[cfg(feature = "subscription-worker")]
    #[doc(hidden)]
    pub fn __websocket_opening<S: crate::__streaming::WebSocketIo>(
        &self,
        seed: ClaimedWebSocketUpgrade<'static>,
        socket: S,
        deadlines: &'static dyn DeadlineFactory,
    ) -> Result<crate::__streaming::WebSocketOpeningDriver<S>, WebSocketSeedError> {
        self.neutral
            .shared_state()
            .websocket_opening(seed, socket, deadlines)
    }

    /// Runs bounded opening checks without reading a body or sending HTTP 101.
    #[doc(hidden)]
    pub fn __admit_websocket_upgrade<'a, 'deadline: 'a>(
        &'a self,
        request: TransportRequest<'a>,
        version: WebSocketHttpVersion,
        deadlines: &'deadline dyn DeadlineFactory,
    ) -> BoxFuture<'a, WebSocketUpgradeIngress<'deadline>> {
        crate::transport::websocket::handle(
            self.neutral.shared_state(),
            request,
            version,
            deadlines,
        )
    }

    /// Consumes a seed and validates its endpoint owner before HTTP 101.
    ///
    /// A foreign seed is dropped with its connection permit and is not returned
    /// for retry. This check moves, but never recreates, the upgrade deadline.
    #[doc(hidden)]
    pub fn __claim_websocket_upgrade<'deadline>(
        &self,
        seed: AdmittedWebSocketUpgrade<'deadline>,
    ) -> Result<ClaimedWebSocketUpgrade<'deadline>, WebSocketSeedError> {
        let owner = self
            .neutral
            .shared_state()
            .subscriptions
            .resources
            .delivery()
            .byte_owner();
        seed.claim(&owner)
    }

    /// Transfers an already-upgraded, same-endpoint seed into initialization.
    ///
    /// Construction starts the sole connection-initialization timer. Polling
    /// still gives the original upgrade deadline deterministic precedence.
    #[cfg(feature = "subscription-worker")]
    #[doc(hidden)]
    pub fn __begin_websocket_connection(
        &self,
        seed: ClaimedWebSocketUpgrade<'static>,
        deadlines: &'static dyn DeadlineFactory,
    ) -> Result<Handoff, WebSocketSeedError> {
        self.neutral
            .shared_state()
            .begin_websocket_connection(seed, deadlines)
    }
}

#[cfg(feature = "subscription-worker")]
impl<C: Send + Sync + 'static> crate::transport::EndpointState<C> {
    pub(crate) fn websocket_session<S: crate::__streaming::WebSocketIo>(
        &self,
        seed: ClaimedWebSocketUpgrade<'static>,
        socket: S,
        deadlines: &'static dyn DeadlineFactory,
    ) -> Result<crate::__streaming::WebSocketSessionDriver<C, S>, WebSocketSeedError> {
        let opening = self.websocket_opening(seed, socket, deadlines)?;
        Ok(crate::__streaming::WebSocketSessionDriver::new(
            opening,
            std::sync::Arc::clone(&self.subscription_execution),
            self.lifecycle.clone(),
            self.subscriptions.resources.delivery().byte_owner(),
        ))
    }

    fn websocket_opening<S: crate::__streaming::WebSocketIo>(
        &self,
        seed: ClaimedWebSocketUpgrade<'static>,
        socket: S,
        deadlines: &'static dyn DeadlineFactory,
    ) -> Result<crate::__streaming::WebSocketOpeningDriver<S>, WebSocketSeedError> {
        let handoff = self.begin_websocket_connection(seed, deadlines)?;
        let timeout = self
            .subscriptions
            .settings
            .limits()
            .send_no_progress_timeout;
        Ok(crate::__streaming::WebSocketOpeningDriver::new(
            handoff, socket, deadlines, timeout,
        ))
    }

    fn begin_websocket_connection(
        &self,
        seed: ClaimedWebSocketUpgrade<'static>,
        deadlines: &'static dyn DeadlineFactory,
    ) -> Result<Handoff, WebSocketSeedError> {
        let telemetry = seed.request_telemetry().clone();
        let owner = self.subscriptions.resources.delivery().byte_owner();
        let parts = seed.into_connection_parts(&owner)?;
        let observer = crate::transport::websocket::observer(self.schema.clone(), telemetry);
        Ok(Handoff::new(
            parts,
            deadlines,
            std::sync::Arc::clone(&self.subscriptions.authorizer),
            observer,
            self.subscription_execution.task_scope().0,
        ))
    }
}
