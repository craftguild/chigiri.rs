//! Transport-neutral subscription lifecycle contracts.

mod client_message;
mod connection_init;
mod control_encoding;
mod event_encoding;
mod http_profile;
mod phase;
pub(crate) mod websocket_upgrade;
mod websocket_write_progress;

pub use client_message::{
    RejectedWebSocketOperationId, WebSocketClientMessage, WebSocketClientMessageError,
    WebSocketClientMessageErrorKind, WebSocketControlPayload, WebSocketObjectPayload,
    WebSocketOperationId, WebSocketSubscribe,
};
pub use connection_init::{
    AcceptConnectionInit, ConnectionInitAuthorizer, ConnectionInitContext, ConnectionInitDecision,
    ConnectionInitError,
};
pub use control_encoding::{
    EncodedSubscriptionControl, SubscriptionControl, SubscriptionControlEncodingError,
    SubscriptionStaticControl,
};
pub use event_encoding::{
    EncodedSubscriptionEvent, FinalizedSubscriptionEvent, SubscriptionEventEncodingError,
};
pub use phase::{RequestPhase, SubscriptionTransport, WebSocketOneShot};
pub use websocket_upgrade::{
    VerifiedWebSocketUpgrade, WebSocketHttpVersion, WebSocketUpgradeError,
};
pub use websocket_write_progress::{
    WebSocketAcceptedTextProgress, WebSocketWriteProgressError, WebSocketWriteProgressTracker,
};

pub(crate) use client_message::decode as decode_client_message;
pub(crate) use control_encoding::{
    encode as encode_control, encode_http_initial, encode_peer_pong,
    encode_rejected_operation_error, peer_pong_len, static_len as static_control_len,
};
pub(crate) use event_encoding::encode as encode_event;
pub(crate) use http_profile::negotiate as negotiate_http_profile;
