//! Endpoint build error and internal support for unit context.

use std::fmt;

use chigiri_http::{BoxFuture, ContextFactory, PrepareError, PreparedContext, RequestContext};
use chigiri_runtime::SchemaBuildError;

use crate::configuration::ConfigurationOrigin;

mod constraint;
mod diagnostic;

pub use constraint::{ConfigurationConstraint, ConfigurationOperand, ConfigurationValue};
pub use diagnostic::ConfigurationDiagnostic;

/// The final validation identifies the setting slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ConfigurationSlot {
    /// GraphQL execution limits.
    ExecutionLimits,
    /// Parsed document cache limits.
    DocumentCache,
    /// The request body byte limit.
    RequestBodyByteLimit,
    /// The response byte limit.
    ResponseByteLimit,
    /// Request body deadline to read.
    BodyTimeout,
    /// Transport-admission deadline.
    TransportAdmissionTimeout,
    /// The deadline for the operation source.
    SourceResolutionTimeout,
    /// Decoded-request-hook deadline.
    DecodedRequestTimeout,
    /// Creates a deadline for context construction.
    ContextTimeout,
    /// GraphQL execution deadline.
    ExecutionTimeout,
    /// Response-hook deadline.
    ResponseHookTimeout,
    /// The entire request deadline.
    OverallTimeout,
    /// Active subscriptions shared by one endpoint.
    ActiveSubscriptions,
    /// Wholly encoded queued events for one subscription.
    QueuedSubscriptionEvents,
    /// Queued and in-flight event bytes for one subscription.
    QueuedSubscriptionBytes,
    /// One encoded subscription event.
    EncodedSubscriptionEventBytes,
    /// Oldest queued subscription event residence time.
    SubscriptionQueueAge,
    /// Subscription send no-progress deadline.
    SubscriptionSendNoProgressTimeout,
    /// Subscription setup deadline.
    SubscriptionSetupTimeout,
    /// Per-event preparation and execution deadline.
    SubscriptionEventTimeout,
    /// Encoded subscription bytes shared by endpoint clones.
    EndpointSubscriptionEncodedBytes,
    /// Pending operation setups shared by endpoint clones.
    PendingOperationSetups,
    /// Retained request metadata for one subscription.
    RetainedSubscriberMetadata,
    /// Subscription heartbeat interval.
    SubscriptionHeartbeatInterval,
    /// Active operations for one multiplexed connection.
    ActiveOperationsPerConnection,
    /// Open WebSocket protocol connections for one endpoint.
    WebSocketConnections,
    /// Pending setup operations for one WebSocket connection.
    PendingSetupsPerConnection,
    /// The reason for the baseline protection.
    SafetyOverrideReason,
}

/// The type of discrepancy that final validation identifies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ConfigurationViolation {
    /// An unbounded value was supplied where a finite limit is required.
    Unbounded,
    /// Only part of a compound slot was disabled.
    PartiallyDisabled,
    /// The value is outside the public hard bound.
    OutsideHardBound,
    /// The deadline is zero.
    ZeroDuration,
    /// The deadline cannot be scheduled safely.
    UnschedulableDuration,
    /// The required audit reason is empty.
    EmptyReason,
    /// Checked arithmetic for a derived bound overflowed.
    ArithmeticOverflow,
    /// Individually valid limits cannot satisfy a required fit relationship.
    InconsistentLimits,
}

/// The endpoint's final build failed.
pub enum EndpointBuildError {
    /// Building the executable schema failed.
    Schema(SchemaBuildError),
    /// The final component-graph configuration is invalid.
    InvalidConfiguration(ConfigurationDiagnostic),
    /// Initializing the request identity factory failed.
    IdentityInitialization,
}

impl fmt::Debug for EndpointBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Schema(error) => formatter.debug_tuple("Schema").field(error).finish(),
            Self::InvalidConfiguration(diagnostic) => formatter
                .debug_tuple("InvalidConfiguration")
                .field(diagnostic)
                .finish(),
            Self::IdentityInitialization => formatter.write_str("IdentityInitialization"),
        }
    }
}

impl EndpointBuildError {
    pub(crate) const fn invalid(
        slot: ConfigurationSlot,
        origin: ConfigurationOrigin,
        violation: ConfigurationViolation,
    ) -> Self {
        Self::InvalidConfiguration(ConfigurationDiagnostic::new(slot, origin, violation))
    }
    pub(crate) const fn identity() -> Self {
        Self::IdentityInitialization
    }
}

impl fmt::Display for EndpointBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Schema(error) => write!(formatter, "schema build failed: {error}"),
            Self::InvalidConfiguration(diagnostic) => {
                write!(formatter, "invalid endpoint configuration: {diagnostic}")
            }
            Self::IdentityInitialization => {
                formatter.write_str("request identity initialization failed")
            }
        }
    }
}

impl std::error::Error for EndpointBuildError {}

pub(crate) struct UnitContextFactory;

impl ContextFactory<(), ()> for UnitContextFactory {
    fn create<'a>(
        &'a self,
        _request: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<(), ()>, PrepareError>> {
        Box::pin(async { Ok(PreparedContext::execution_only(())) })
    }
}
