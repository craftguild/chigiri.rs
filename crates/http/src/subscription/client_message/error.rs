use std::fmt;

/// Safe classification for a rejected WebSocket client message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub enum WebSocketClientMessageErrorKind {
    /// The complete UTF-8 text message exceeded one MiB.
    MessageTooLarge,
    /// JSON syntax, direction, duplicate members, or protocol shape was invalid.
    MalformedMessage,
    /// An init, ping, or pong JSON payload exceeded 16 KiB.
    PayloadTooLarge,
    /// An otherwise identifiable operation used an ID longer than 256 UTF-8 bytes.
    OperationIdTooLarge,
}

/// An oversized operation ID retained only for the typed rejection path.
///
/// Its value is intentionally absent from `Debug` and error text.
#[doc(hidden)]
pub struct RejectedWebSocketOperationId {
    value: String,
}

impl RejectedWebSocketOperationId {
    /// Borrows the rejected ID for the dedicated bounded error encoder.
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Transfers the rejected ID to its connection state owner.
    pub fn into_string(self) -> String {
        self.value
    }
}

impl AsRef<str> for RejectedWebSocketOperationId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for RejectedWebSocketOperationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RejectedWebSocketOperationId")
            .field("bytes", &self.value.len())
            .finish_non_exhaustive()
    }
}

/// A redacted failure from the pure bounded client-message decoder.
#[doc(hidden)]
pub struct WebSocketClientMessageError {
    kind: WebSocketClientMessageErrorKind,
    operation_id: Option<RejectedWebSocketOperationId>,
}

impl WebSocketClientMessageError {
    /// Returns the safe action classification.
    pub const fn kind(&self) -> WebSocketClientMessageErrorKind {
        self.kind
    }

    /// Borrows an otherwise valid oversized operation ID, when identifiable.
    pub fn operation_id(&self) -> Option<&RejectedWebSocketOperationId> {
        self.operation_id.as_ref()
    }

    /// Transfers an otherwise valid oversized operation ID to the state owner.
    pub fn into_operation_id(self) -> Option<RejectedWebSocketOperationId> {
        self.operation_id
    }

    pub(super) const fn message_too_large() -> Self {
        Self::plain(WebSocketClientMessageErrorKind::MessageTooLarge)
    }

    pub(super) const fn malformed() -> Self {
        Self::plain(WebSocketClientMessageErrorKind::MalformedMessage)
    }

    pub(super) const fn payload_too_large() -> Self {
        Self::plain(WebSocketClientMessageErrorKind::PayloadTooLarge)
    }

    pub(super) fn operation_id_too_large(operation_id: String) -> Self {
        Self {
            kind: WebSocketClientMessageErrorKind::OperationIdTooLarge,
            operation_id: Some(RejectedWebSocketOperationId {
                value: operation_id,
            }),
        }
    }

    const fn plain(kind: WebSocketClientMessageErrorKind) -> Self {
        Self {
            kind,
            operation_id: None,
        }
    }
}

impl fmt::Debug for WebSocketClientMessageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebSocketClientMessageError")
            .field("kind", &self.kind)
            .field(
                "operation_id_bytes",
                &self.operation_id.as_ref().map(|value| value.value.len()),
            )
            .finish_non_exhaustive()
    }
}

impl fmt::Display for WebSocketClientMessageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self.kind {
            WebSocketClientMessageErrorKind::MessageTooLarge => {
                "WebSocket client message exceeds its byte limit"
            }
            WebSocketClientMessageErrorKind::MalformedMessage => {
                "WebSocket client message is malformed"
            }
            WebSocketClientMessageErrorKind::PayloadTooLarge => {
                "WebSocket client payload exceeds its byte limit"
            }
            WebSocketClientMessageErrorKind::OperationIdTooLarge => {
                "WebSocket operation ID exceeds its byte limit"
            }
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for WebSocketClientMessageError {}
