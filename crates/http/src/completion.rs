//! Finalizes request-scoped resources once response encoding and transport handling complete.

use crate::ResponseParts;

/// Transport-independent outcome of response preparation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResponseCompletionOutcome {
    /// Encoding completed within the shared deadline before transport handoff.
    Encoded,
    /// A response hook or post-execution encoding failed.
    Failed,
    /// Encoding exceeded its byte or node limit.
    EncodingLimitExceeded,
    /// The deadline elapsed during execution or response preparation.
    TimedOut,
}

/// Completes or rejects transport delivery exactly once for each request.
///
/// Implementations derive success from the transport's completion state, not only its HTTP
/// status. Callers invoke this after bounded response preparation and before unbounded transport
/// waiting or user code. Dropping the transport must still cancel or release pending work.
pub trait ResponseCompletion: Send + Sync + 'static {
    /// Return response parts updated with the final preparation and transport outcome.
    fn complete(
        &self,
        response: ResponseParts,
        outcome: ResponseCompletionOutcome,
    ) -> ResponseParts;
}
