//! Bounded pre-upgrade admission with no socket or hidden runtime ownership.

#[cfg(feature = "subscription-worker")]
mod connection;
mod ingress;
#[cfg(feature = "subscription-worker")]
mod managed_upgrade;
mod observation;

pub(crate) use ingress::handle;
mod request;
#[cfg(feature = "subscription-worker")]
mod session;
#[cfg(test)]
mod tests;

use std::fmt;
use std::task::{Context, Poll};

use chigiri_http::{BoxFuture, ResponseParts, TransportRequest};
use chigiri_runtime::RequestTelemetry;

use crate::subscription::bridge_resources::{BridgeConnection, BridgeGeneration};
use crate::subscription::delivery::EndpointByteOwner;
#[cfg(feature = "subscription-worker")]
pub(crate) use managed_upgrade::RetainedWebSocketUpgrade;
use request::AdmittedWebSocketRequest;

#[cfg(feature = "subscription-worker")]
pub use connection::{
    AckPendingWebSocketConnection, InitializedWebSocketConnection, InitializedWebSocketInput,
    WebSocketAcknowledgementFrame, WebSocketAcknowledgementProgress,
    WebSocketAcknowledgementResolution, WebSocketConnectionCleanup, WebSocketConnectionClose,
    WebSocketConnectionFrame, WebSocketConnectionHandoff, WebSocketConnectionHandoffResolution,
    WebSocketConnectionInitialization, WebSocketConnectionResolution, WebSocketInitializationInput,
};
#[cfg(feature = "subscription-worker")]
pub(crate) use connection::{
    AuthorizationErrorObserver, Handoff, InitializedWebSocketConnectionParts,
    native_socket_observer, observer,
};
#[cfg(feature = "subscription-worker")]
pub use session::{
    WebSocketInbound, WebSocketIo, WebSocketIoError, WebSocketOpeningDriver,
    WebSocketOpeningOutcome, WebSocketSessionDriver, WebSocketWireFrame, WebSocketWriteProgress,
};

const PROTOCOL: &str = "graphql-transport-ws";

/// One finite response or a bounded seed awaiting owner validation before 101.
#[doc(hidden)]
pub enum WebSocketUpgradeIngress<'deadline> {
    /// A complete, credential-free rejection response.
    Finite(ResponseParts),
    /// A move-only seed that still requires same-endpoint consuming validation.
    Admitted(AdmittedWebSocketUpgrade<'deadline>),
}

/// Move-only admitted seed before its endpoint owner is consumed and checked.
///
/// ```compile_fail
/// use chigiri::__streaming::AdmittedWebSocketUpgrade;
///
/// fn duplicate(seed: AdmittedWebSocketUpgrade<'_>) {
///     let _copy = seed.clone();
/// }
/// ```
#[doc(hidden)]
pub struct AdmittedWebSocketUpgrade<'deadline> {
    request: Box<AdmittedWebSocketRequest>,
    connection: BridgeConnection,
    owner: EndpointByteOwner,
    deadline: BoxFuture<'deadline, ()>,
}

/// Same-endpoint checked seed required before an application can send HTTP 101.
#[doc(hidden)]
pub struct ClaimedWebSocketUpgrade<'deadline> {
    request: AdmittedWebSocketRequest,
    connection: BridgeConnection,
    #[cfg(feature = "subscription-worker")]
    owner: EndpointByteOwner,
    deadline: BoxFuture<'deadline, ()>,
}

#[cfg(feature = "subscription-worker")]
pub(crate) struct ClaimedConnectionParts {
    request: AdmittedWebSocketRequest,
    connection: BridgeConnection,
    owner: EndpointByteOwner,
    deadline: BoxFuture<'static, ()>,
}

/// Closed reason that an admitted seed cannot enter a connection session.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebSocketSeedError {
    /// The seed was issued by a different independently built endpoint.
    ForeignEndpoint,
}

impl fmt::Display for WebSocketSeedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WebSocket upgrade seed belongs to another endpoint")
    }
}

impl std::error::Error for WebSocketSeedError {}

impl<'deadline> AdmittedWebSocketUpgrade<'deadline> {
    /// Cancels before endpoint-owner validation or HTTP 101.
    pub fn cancel(self) {
        drop(self);
    }

    pub(crate) fn claim(
        self,
        expected: &EndpointByteOwner,
    ) -> Result<ClaimedWebSocketUpgrade<'deadline>, WebSocketSeedError> {
        if !self.owner.same_owner(expected) {
            drop(self);
            return Err(WebSocketSeedError::ForeignEndpoint);
        }
        let Self {
            request,
            connection,
            owner,
            deadline,
        } = self;
        #[cfg(not(feature = "subscription-worker"))]
        drop(owner);
        Ok(ClaimedWebSocketUpgrade {
            request: *request,
            connection,
            #[cfg(feature = "subscription-worker")]
            owner,
            deadline,
        })
    }
}

impl ClaimedWebSocketUpgrade<'_> {
    #[cfg(feature = "subscription-worker")]
    pub(crate) fn instrumentation(
        &self,
    ) -> Option<&chigiri_runtime::instrumentation::CapturedInstrumentation> {
        self.request.instrumentation()
    }

    /// Returns the validated nonce for the reviewed accept-key calculation.
    pub fn key(&self) -> &str {
        self.request.key()
    }

    /// Returns the sole negotiated GraphQL WebSocket subprotocol.
    pub const fn protocol(&self) -> &'static str {
        PROTOCOL
    }

    /// Returns the retained request correlation and sampling snapshot.
    pub const fn request_telemetry(&self) -> &RequestTelemetry {
        self.request.telemetry()
    }

    /// Returns the exact bounded retained-metadata charge for this seed.
    pub const fn charged_bytes(&self) -> usize {
        self.request.charged_bytes()
    }

    /// Inspects the retained transport through a borrow that cannot escape.
    pub fn inspect_transport<R>(&self, inspect: impl FnOnce(TransportRequest<'_>) -> R) -> R {
        self.request.inspect_transport(inspect)
    }

    /// Returns the exact generation of the connection permit acquired before 101.
    pub fn generation(&self) -> BridgeGeneration {
        self.connection.generation()
    }

    /// Polls the original ten-second upgrade budget without creating a new timer.
    pub fn poll_upgrade_deadline(&mut self, context: &mut Context<'_>) -> Poll<()> {
        self.deadline.as_mut().poll(context)
    }

    /// Cancels before the application transfers this seed to the connection driver.
    pub fn cancel(self) {
        drop(self);
    }
}

#[cfg(feature = "subscription-worker")]
impl ClaimedWebSocketUpgrade<'static> {
    pub(crate) fn into_connection_parts(
        self,
        expected: &EndpointByteOwner,
    ) -> Result<ClaimedConnectionParts, WebSocketSeedError> {
        if !self.owner.same_owner(expected) {
            drop(self);
            return Err(WebSocketSeedError::ForeignEndpoint);
        }
        let Self {
            request,
            connection,
            owner,
            deadline,
        } = self;
        Ok(ClaimedConnectionParts {
            request,
            connection,
            owner,
            deadline,
        })
    }
}

impl fmt::Debug for AdmittedWebSocketUpgrade<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdmittedWebSocketUpgrade")
            .field("protocol", &PROTOCOL)
            .field("charged_bytes", &self.request.charged_bytes())
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for ClaimedWebSocketUpgrade<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClaimedWebSocketUpgrade")
            .field("protocol", &PROTOCOL)
            .field("charged_bytes", &self.request.charged_bytes())
            .finish_non_exhaustive()
    }
}
