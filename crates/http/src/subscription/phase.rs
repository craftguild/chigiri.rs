//! Closed context-preparation phases for finite and streaming operations.

/// A protocol profile that can carry a GraphQL subscription.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubscriptionTransport {
    /// The `graphql-transport-ws` WebSocket protocol.
    WebSocket,
    /// The distinct-connections GraphQL-over-SSE profile.
    ServerSentEvents,
    /// The Apollo `subscriptionSpec=1.0` multipart profile.
    Multipart,
}

/// A finite operation carried inside an established WebSocket connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebSocketOneShot {
    /// A GraphQL Query operation.
    Query,
    /// A GraphQL Mutation operation.
    Mutation,
}

/// The closed lifecycle phase supplied to an endpoint context factory.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RequestPhase {
    /// An ordinary finite HTTP request.
    #[default]
    Finite,
    /// A finite Query or Mutation on an established WebSocket connection.
    WebSocketOperation {
        /// The finite operation kind admitted on the connection.
        operation: WebSocketOneShot,
    },
    /// Subscription source initialization.
    SubscriptionSetup {
        /// The protocol profile that owns this subscription.
        transport: SubscriptionTransport,
    },
    /// Execution of one source event in sequence.
    SubscriptionEvent {
        /// The protocol profile that owns this subscription.
        transport: SubscriptionTransport,
        /// The monotonically increasing event sequence assigned by the owner.
        sequence: u64,
    },
}

impl RequestPhase {
    pub(crate) const fn retains_raw_body(self) -> bool {
        matches!(self, Self::Finite)
    }

    pub(crate) const fn exposes_connection_init(self) -> bool {
        matches!(self, Self::WebSocketOperation { .. })
            || matches!(
                self,
                Self::SubscriptionSetup {
                    transport: SubscriptionTransport::WebSocket
                } | Self::SubscriptionEvent {
                    transport: SubscriptionTransport::WebSocket,
                    ..
                }
            )
    }
}
