//! Immutable configuration values for bounded subscription delivery.

use std::time::Duration;

/// Endpoint-wide and per-subscription delivery limits.
///
/// The complete value replaces the previous setting. Every field is validated
/// together during [`crate::EndpointBuilder::build`].
/// One encoded event must fit both `max_queued_bytes` and
/// `max_endpoint_encoded_bytes`. Queue byte and event-count caps are independent;
/// the queue need not hold `max_queued_events` maximum-size events. Individual
/// hard maxima do not promise simultaneous capacity. See the
/// [whole-configuration walkthrough](https://github.com/craftguild/chigiri.rs/blob/main/docs/subscriptions-walkthrough.md#whole-configuration-sizing)
/// for complete profile framing and shared-buffer headroom. One maximum-event
/// reservation precedes source demand; shared setup and control bytes also need
/// room beyond queued/in-flight event storage.
/// Counts and durations retain their Rust types before numeric validation:
///
/// ```compile_fail
/// use chigiri::SubscriptionLimits;
/// let limits = SubscriptionLimits {
///     max_queued_events: "32",
///     ..SubscriptionLimits::default()
/// };
/// ```
///
/// ```compile_fail
/// use chigiri::SubscriptionLimits;
/// let limits = SubscriptionLimits {
///     max_queue_age: 10,
///     ..SubscriptionLimits::default()
/// };
/// ```
///
/// Builder sugar also requires the complete typed value:
///
/// ```compile_fail
/// fn configure(builder: chigiri::MissingContextEndpointBuilder<()>) {
///     let _ = builder.subscription_limits(32);
/// }
/// ```
///
/// Correctly typed values use the same slots through sugar and injection:
///
/// ```
/// use chigiri::{MissingContextEndpointBuilder, SubscriptionLimits, SubscriptionProtocolSettings};
/// use std::time::Duration;
/// fn configure(builder: MissingContextEndpointBuilder<()>) -> MissingContextEndpointBuilder<()> {
///     let limits = SubscriptionLimits {
///         max_queued_events: 32,
///         max_queue_age: Duration::from_secs(10),
///         ..SubscriptionLimits::default()
///     };
///     builder.subscription_limits(limits).inject(|components| {
///         components.subscriptions().protocol(SubscriptionProtocolSettings::default());
///     })
/// }
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SubscriptionLimits {
    /// Maximum active subscriptions shared by all clones of one endpoint.
    pub max_active_subscriptions: usize,
    /// Maximum wholly encoded queued events; excludes the one in-flight entry.
    pub max_queued_events: usize,
    /// Maximum queued and in-flight encoded event bytes for one subscription.
    pub max_queued_bytes: usize,
    /// Maximum bytes for one event, including delivery metadata and profile framing.
    pub max_encoded_event_bytes: usize,
    /// Maximum residence time for the oldest queued event.
    pub max_queue_age: Duration,
    /// Maximum time that a pending send may make no byte progress.
    pub send_no_progress_timeout: Duration,
    /// Maximum subscription preparation and source-initialization time.
    pub setup_timeout: Duration,
    /// Maximum preparation and execution time for each event.
    pub event_timeout: Duration,
    /// Maximum encoded bytes shared by every clone of one endpoint.
    pub max_endpoint_encoded_bytes: usize,
    /// Maximum concurrent pending operation setups for one endpoint.
    pub max_pending_operation_setups: usize,
    /// Maximum retained subscriber request metadata for one subscription.
    pub max_retained_metadata_bytes: usize,
    /// Heartbeat interval for a transport profile that supports heartbeats.
    pub heartbeat_interval: Duration,
}

impl SubscriptionLimits {
    /// Inclusive minimum for every count field.
    pub const MIN_COUNT: usize = 1;
    /// Inclusive minimum for every byte field, in bytes.
    pub const MIN_BYTES: usize = 1;
    /// Inclusive minimum for queue, send, setup, and event durations.
    pub const MIN_TIMEOUT: Duration = Duration::from_nanos(1);
    /// Inclusive maximum active subscriptions per endpoint.
    pub const MAX_ACTIVE_SUBSCRIPTIONS: usize = 1_024;
    /// Inclusive maximum queued event count per subscription.
    pub const MAX_QUEUED_EVENTS: usize = 1_024;
    /// Inclusive maximum queued and in-flight bytes per subscription (4 MiB).
    pub const MAX_QUEUED_BYTES: usize = 4 * 1024 * 1024;
    /// Inclusive maximum complete profile-encoded event bytes (1 MiB).
    pub const MAX_ENCODED_EVENT_BYTES: usize = 1024 * 1024;
    /// Inclusive maximum oldest queued event residence time.
    pub const MAX_QUEUE_AGE: Duration = Duration::from_secs(60);
    /// Inclusive maximum pending-send no-progress duration.
    pub const MAX_SEND_NO_PROGRESS_TIMEOUT: Duration = Duration::from_secs(60);
    /// Inclusive maximum setup duration.
    pub const MAX_SETUP_TIMEOUT: Duration = Duration::from_secs(60);
    /// Inclusive maximum per-event preparation and execution duration.
    pub const MAX_EVENT_TIMEOUT: Duration = Duration::from_secs(120);
    /// Inclusive maximum shared encoded-buffer bytes per endpoint (256 MiB).
    pub const MAX_ENDPOINT_ENCODED_BYTES: usize = 256 * 1024 * 1024;
    /// Inclusive maximum concurrent pending operation setups per endpoint.
    pub const MAX_PENDING_OPERATION_SETUPS: usize = 128;
    /// Inclusive maximum retained subscriber request metadata bytes (64 KiB).
    pub const MAX_RETAINED_METADATA_BYTES: usize = 64 * 1024;
    /// Inclusive minimum heartbeat interval.
    pub const MIN_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);
    /// Inclusive maximum heartbeat interval.
    pub const MAX_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(60);
}

impl Default for SubscriptionLimits {
    fn default() -> Self {
        Self {
            max_active_subscriptions: 128,
            max_queued_events: 32,
            max_queued_bytes: 256 * 1024,
            max_encoded_event_bytes: 64 * 1024,
            max_queue_age: Duration::from_secs(10),
            send_no_progress_timeout: Duration::from_secs(10),
            setup_timeout: Duration::from_secs(10),
            event_timeout: Duration::from_secs(30),
            max_endpoint_encoded_bytes: 64 * 1024 * 1024,
            max_pending_operation_setups: 16,
            max_retained_metadata_bytes: 16 * 1024,
            heartbeat_interval: Duration::from_secs(15),
        }
    }
}

/// Quotas specific to multiplexed WebSocket protocol ownership.
///
/// The complete value replaces the previous setting during endpoint building.
/// A protocol quota is a count, not an unparsed configuration string:
///
/// ```compile_fail
/// use chigiri::SubscriptionProtocolSettings;
/// let protocol = SubscriptionProtocolSettings {
///     max_websocket_connections: "64",
///     ..SubscriptionProtocolSettings::default()
/// };
/// ```
///
/// Typed injection does not accept a delivery-limits value in the protocol slot:
///
/// ```compile_fail
/// fn configure(builder: chigiri::MissingContextEndpointBuilder<()>) {
///     let _ = builder.inject(|components| {
///         components.subscriptions().protocol(chigiri::SubscriptionLimits::default());
///     });
/// }
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SubscriptionProtocolSettings {
    /// Maximum active Query, Mutation, and Subscription operations per connection.
    pub max_active_operations_per_connection: usize,
    /// Maximum open Chigiri WebSocket protocol connections per endpoint.
    pub max_websocket_connections: usize,
    /// Maximum concurrent pending setup operations per WebSocket connection.
    pub max_pending_setups_per_connection: usize,
}

impl SubscriptionProtocolSettings {
    /// Inclusive minimum for every protocol count field.
    pub const MIN_COUNT: usize = SubscriptionLimits::MIN_COUNT;
    /// Inclusive maximum active operations per multiplexed connection.
    pub const MAX_ACTIVE_OPERATIONS_PER_CONNECTION: usize = 128;
    /// Inclusive maximum open WebSocket connections per endpoint.
    pub const MAX_WEBSOCKET_CONNECTIONS: usize = 512;
    /// Inclusive maximum pending setup operations per WebSocket connection.
    pub const MAX_PENDING_SETUPS_PER_CONNECTION: usize = 32;
}

impl Default for SubscriptionProtocolSettings {
    fn default() -> Self {
        Self {
            max_active_operations_per_connection: 16,
            max_websocket_connections: 64,
            max_pending_setups_per_connection: 8,
        }
    }
}

/// Redacted description of the WebSocket initialization authorization slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionInitAuthorization {
    /// The automatic policy accepts after ordinary transport admission.
    DefaultAccept,
    /// The application replaced the policy with a callback.
    ApplicationDefined,
}

/// Validated settings passed to the shared subscription resource owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ValidatedSubscriptionSettings {
    limits: SubscriptionLimits,
    protocol: SubscriptionProtocolSettings,
}

impl ValidatedSubscriptionSettings {
    pub(crate) const fn new(
        limits: SubscriptionLimits,
        protocol: SubscriptionProtocolSettings,
    ) -> Self {
        Self { limits, protocol }
    }

    pub(crate) const fn limits(&self) -> &SubscriptionLimits {
        &self.limits
    }

    pub(crate) const fn protocol(&self) -> &SubscriptionProtocolSettings {
        &self.protocol
    }
}

#[cfg(test)]
mod tests;
