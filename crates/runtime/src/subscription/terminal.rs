//! Retain a typed execution decision independently of its safe GraphQL response.

use std::ops::Deref;

use chigiri_core::GraphQlResponse;

use super::SubscriptionTermination;

/// Closed runtime terminal output; response text never determines its cause.
#[derive(Debug)]
#[doc(hidden)]
pub struct SubscriptionTerminalResponse {
    response: GraphQlResponse,
    decision: SubscriptionTermination,
}

impl SubscriptionTerminalResponse {
    /// A source item, event preparation, policy, budget, or execution failed.
    pub const fn execution(response: GraphQlResponse) -> Self {
        Self {
            response,
            decision: SubscriptionTermination::ExecutionFailed,
        }
    }

    /// An ownership or runtime invariant failed, rather than application work.
    pub const fn internal(response: GraphQlResponse) -> Self {
        Self {
            response,
            decision: SubscriptionTermination::Internal,
        }
    }

    /// Moves the safe response and the separately selected typed decision.
    pub fn into_parts(self) -> (GraphQlResponse, SubscriptionTermination) {
        (self.response, self.decision)
    }

    /// Removes only internal decision metadata for an ordinary response consumer.
    pub fn into_response(self) -> GraphQlResponse {
        self.response
    }

    /// Returns the immutable decision without inspecting payloads or extensions.
    pub const fn decision(&self) -> SubscriptionTermination {
        self.decision
    }
}

impl Deref for SubscriptionTerminalResponse {
    type Target = GraphQlResponse;
    fn deref(&self) -> &Self::Target {
        &self.response
    }
}
