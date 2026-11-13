//! Closed decoded input handed from initialization to the operation session.

use chigiri_http::{RejectedWebSocketOperationId, WebSocketOperationId, WebSocketSubscribe};

/// A pre-ack message effect that never admits a GraphQL operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub enum WebSocketInitializationInput {
    /// A ping response was queued in the bounded control staging.
    ControlQueued,
    /// A peer pong was observed without treating it as send progress.
    PongObserved,
    /// A harmless completion for no admitted operation was ignored.
    Ignored,
    /// The first initialization message started authorization.
    AuthorizationStarted,
}

/// A post-ack message classified before any operation worker is admitted.
#[doc(hidden)]
pub enum InitializedWebSocketInput {
    /// One decoded Query, Mutation, or Subscription request candidate.
    Subscribe(WebSocketSubscribe),
    /// One client completion for an operation ID.
    Complete(WebSocketOperationId),
    /// One safely identified oversized operation ID for bounded error encoding.
    RejectedOperationId(RejectedWebSocketOperationId),
    /// A ping response was queued in the bounded control staging.
    ControlQueued,
    /// A peer pong was observed without treating it as send progress.
    PongObserved,
}
