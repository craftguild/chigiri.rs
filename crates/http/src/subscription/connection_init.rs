//! Bounded WebSocket initialization authorization without server-library types.

use std::error::Error;
use std::fmt;
use std::net::SocketAddr;

use chigiri_runtime::Object;

use crate::{ApplicationMetadata, BoxFuture, RequestHeader, TransportRequest};

/// Read-only admitted transport data used to authorize `connection_init`.
#[derive(Clone, Copy)]
pub struct ConnectionInitContext<'a> {
    headers: &'a [RequestHeader<'a>],
    application_metadata: &'a ApplicationMetadata,
    peer_addr: Option<SocketAddr>,
    payload: Option<&'a Object>,
}

impl<'a> ConnectionInitContext<'a> {
    /// Creates the restricted authorization view from an admitted request.
    #[doc(hidden)]
    pub const fn __new(request: TransportRequest<'a>, payload: Option<&'a Object>) -> Self {
        Self {
            headers: request.headers(),
            application_metadata: request.application_metadata(),
            peer_addr: request.peer_addr(),
            payload,
        }
    }

    /// Returns the admitted HTTP request headers.
    pub const fn headers(&self) -> &'a [RequestHeader<'a>] {
        self.headers
    }

    /// Returns bounded application metadata supplied by the adapter.
    pub const fn application_metadata(&self) -> &'a ApplicationMetadata {
        self.application_metadata
    }

    /// Returns the admitted peer address when the adapter supplied one.
    pub const fn peer_addr(&self) -> Option<SocketAddr> {
        self.peer_addr
    }

    /// Returns the optional bounded initialization object.
    pub const fn payload(&self) -> Option<&'a Object> {
        self.payload
    }
}

impl fmt::Debug for ConnectionInitContext<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectionInitContext")
            .field("header_count", &self.headers.len())
            .field("metadata_count", &self.application_metadata.fields().len())
            .field("has_peer_addr", &self.peer_addr.is_some())
            .field("has_payload", &self.payload.is_some())
            .finish()
    }
}

/// Application decision for an admitted WebSocket initialization payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionInitDecision {
    /// Acknowledge initialization and permit operation admission.
    Accept,
    /// Reject initialization with the protocol's fixed safe reason.
    Reject,
}

/// Opaque application failure while authorizing `connection_init`.
pub struct ConnectionInitError(Box<dyn Error + Send + Sync>);

impl ConnectionInitError {
    /// Retains an internal source without exposing it through formatting.
    pub fn new(source: impl Error + Send + Sync + 'static) -> Self {
        Self(Box::new(source))
    }
}

impl fmt::Debug for ConnectionInitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectionInitError")
            .finish_non_exhaustive()
    }
}

impl fmt::Display for ConnectionInitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WebSocket connection initialization authorization failed")
    }
}

impl Error for ConnectionInitError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.0.as_ref())
    }
}

/// Authorizes one bounded WebSocket `connection_init` message.
pub trait ConnectionInitAuthorizer: Send + Sync + 'static {
    /// Returns an application decision without creating an execution context.
    fn authorize<'a>(
        &'a self,
        request: ConnectionInitContext<'a>,
    ) -> BoxFuture<'a, Result<ConnectionInitDecision, ConnectionInitError>>;
}

impl<F> ConnectionInitAuthorizer for F
where
    F: Send
        + Sync
        + 'static
        + for<'a> Fn(
            ConnectionInitContext<'a>,
        ) -> BoxFuture<'a, Result<ConnectionInitDecision, ConnectionInitError>>,
{
    fn authorize<'a>(
        &'a self,
        request: ConnectionInitContext<'a>,
    ) -> BoxFuture<'a, Result<ConnectionInitDecision, ConnectionInitError>> {
        self(request)
    }
}

/// Default initialization policy that accepts after ordinary admission.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AcceptConnectionInit;

impl ConnectionInitAuthorizer for AcceptConnectionInit {
    fn authorize<'a>(
        &'a self,
        _request: ConnectionInitContext<'a>,
    ) -> BoxFuture<'a, Result<ConnectionInitDecision, ConnectionInitError>> {
        Box::pin(async { Ok(ConnectionInitDecision::Accept) })
    }
}
