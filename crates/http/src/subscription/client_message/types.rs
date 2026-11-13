use std::fmt;
use std::sync::Arc;

use chigiri_core::Object;

use crate::RequestEnvelope;

/// One validated client-to-server message for `graphql-transport-ws`.
#[doc(hidden)]
pub enum WebSocketClientMessage {
    /// A connection initialization request.
    ConnectionInit(WebSocketObjectPayload),
    /// A GraphQL operation request.
    Subscribe(WebSocketSubscribe),
    /// A client operation completion.
    Complete(WebSocketOperationId),
    /// A client JSON ping.
    Ping(WebSocketControlPayload),
    /// A client JSON pong observation.
    Pong(WebSocketControlPayload),
}

impl fmt::Debug for WebSocketClientMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConnectionInit(payload) => formatter
                .debug_tuple("ConnectionInit")
                .field(payload)
                .finish(),
            Self::Subscribe(subscribe) => {
                formatter.debug_tuple("Subscribe").field(subscribe).finish()
            }
            Self::Complete(operation_id) => formatter
                .debug_tuple("Complete")
                .field(operation_id)
                .finish(),
            Self::Ping(payload) => formatter.debug_tuple("Ping").field(payload).finish(),
            Self::Pong(payload) => formatter.debug_tuple("Pong").field(payload).finish(),
        }
    }
}

/// An immutable optional initialization object.
#[doc(hidden)]
pub struct WebSocketObjectPayload {
    object: Option<Arc<Object>>,
}

/// An immutable decoder-owned object payload from a peer ping or pong.
///
/// The validated raw object remains bounded by the protocol's encoded input
/// limit. Its contents are intentionally available only to the dedicated peer
/// pong encoder, preventing arbitrary raw JSON injection.
#[doc(hidden)]
pub struct WebSocketControlPayload {
    raw_object: Option<Box<str>>,
}

impl WebSocketControlPayload {
    /// Returns whether the peer supplied an object instead of null or omission.
    pub fn has_object(&self) -> bool {
        self.raw_object.is_some()
    }

    /// Returns the retained encoded object length without exposing its value.
    pub fn encoded_bytes(&self) -> usize {
        self.raw_object.as_deref().map_or(0, str::len)
    }

    pub(super) fn new(raw_object: Option<Box<str>>) -> Self {
        Self { raw_object }
    }

    pub(in crate::subscription) fn raw_object(&self) -> Option<&str> {
        self.raw_object.as_deref()
    }
}

impl fmt::Debug for WebSocketControlPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebSocketControlPayload")
            .field("has_object", &self.has_object())
            .field("encoded_bytes", &self.encoded_bytes())
            .finish_non_exhaustive()
    }
}

impl WebSocketObjectPayload {
    /// Borrows the accepted object. Omitted and JSON null payloads return `None`.
    pub fn object(&self) -> Option<&Object> {
        self.object.as_deref()
    }

    /// Transfers the immutable object owner without copying its values.
    pub fn into_arc(self) -> Option<Arc<Object>> {
        self.object
    }

    pub(super) const fn new(object: Option<Arc<Object>>) -> Self {
        Self { object }
    }
}

impl fmt::Debug for WebSocketObjectPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebSocketObjectPayload")
            .field("has_object", &self.object.is_some())
            .field("members", &self.object.as_ref().map(|object| object.len()))
            .finish_non_exhaustive()
    }
}

/// One accepted nonempty operation ID of at most 256 UTF-8 bytes.
#[doc(hidden)]
pub struct WebSocketOperationId {
    value: String,
}

impl WebSocketOperationId {
    /// Borrows the accepted ID.
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Transfers the accepted ID to the connection state owner.
    pub fn into_string(self) -> String {
        self.value
    }

    pub(super) const fn new(value: String) -> Self {
        Self { value }
    }
}

impl AsRef<str> for WebSocketOperationId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for WebSocketOperationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebSocketOperationId")
            .field("bytes", &self.value.len())
            .finish_non_exhaustive()
    }
}

/// One accepted subscribe request with its normalized request envelope.
#[doc(hidden)]
pub struct WebSocketSubscribe {
    operation_id: WebSocketOperationId,
    envelope: Box<RequestEnvelope>,
}

impl WebSocketSubscribe {
    /// Borrows the accepted operation ID.
    pub const fn operation_id(&self) -> &WebSocketOperationId {
        &self.operation_id
    }

    /// Borrows the normalized GraphQL request envelope.
    pub const fn envelope(&self) -> &RequestEnvelope {
        &self.envelope
    }

    /// Transfers both accepted values to the connection state owner.
    pub fn into_parts(self) -> (WebSocketOperationId, RequestEnvelope) {
        (self.operation_id, *self.envelope)
    }

    pub(super) fn new(operation_id: WebSocketOperationId, envelope: RequestEnvelope) -> Self {
        Self {
            operation_id,
            envelope: Box::new(envelope),
        }
    }
}

impl fmt::Debug for WebSocketSubscribe {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebSocketSubscribe")
            .field("operation_id", &self.operation_id)
            .field("envelope", &self.envelope)
            .finish()
    }
}
