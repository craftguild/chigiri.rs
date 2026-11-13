//! Secret-free snapshots of effective endpoint configuration.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use chigiri_policy::ProviderFailurePolicy;
use chigiri_rate::RatePolicy;
use chigiri_runtime::{DocumentCacheLimits, ExecutionLimits};

use crate::subscription::settings::{
    ConnectionInitAuthorization, SubscriptionLimits, SubscriptionProtocolSettings,
};

/// Effective value together with its source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationOrigin {
    /// The value inherited from the automatic safe baseline.
    Baseline,
    /// The value that the application has explicitly overwritten.
    User,
}

/// Read-only slot exposing baseline and final values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingSnapshot<T> {
    baseline: T,
    effective: T,
    origin: ConfigurationOrigin,
}

impl<T> SettingSnapshot<T> {
    pub(crate) fn new(baseline: T, effective: T, origin: ConfigurationOrigin) -> Self {
        Self {
            baseline,
            effective,
            origin,
        }
    }

    /// Returns the automatic safe baseline.
    pub const fn baseline(&self) -> &T {
        &self.baseline
    }
    /// Returns the final endpoint value.
    pub const fn effective(&self) -> &T {
        &self.effective
    }
    /// Returns the source of the final value.
    pub const fn origin(&self) -> ConfigurationOrigin {
        self.origin
    }
}

/// Rate-limit stages configured by the application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RateLimitCoverage {
    /// The application-specific rate limit is not configured.
    Unconfigured,
    /// HTTP transport only Stage A is configured.
    Transport,
    /// Stage B is configured only after operation decoding.
    Operation,
    /// Stage A and stage B are both configured.
    Both,
}

/// Final redacted rate-limit configuration applied to one stage.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct RateLimitStageConfiguration {
    provider_timeout: Duration,
    failure_policy: ProviderFailurePolicy,
    policy: RatePolicy,
}

impl fmt::Debug for RateLimitStageConfiguration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RateLimitStageConfiguration")
            .field("failure_policy", &self.failure_policy)
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl RateLimitStageConfiguration {
    pub(crate) const fn new(
        provider_timeout: Duration,
        failure_policy: ProviderFailurePolicy,
        policy: RatePolicy,
    ) -> Self {
        Self {
            provider_timeout,
            failure_policy,
            policy,
        }
    }
    /// Provider-decision deadline.
    pub const fn provider_timeout(self) -> Duration {
        self.provider_timeout
    }
    /// Policy applied after a provider failure.
    pub const fn failure_policy(self) -> ProviderFailurePolicy {
        self.failure_policy
    }
    /// Returns the quota value exposed by the algorithm-specific policy.
    pub const fn policy(self) -> RatePolicy {
        self.policy
    }
}

/// Record of an explicitly removed baseline protection.
#[derive(Clone, Eq, PartialEq)]
pub struct SafetyOverride {
    protection: &'static str,
    reason: Arc<str>,
}

impl fmt::Debug for SafetyOverride {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafetyOverride")
            .field("protection", &self.protection)
            .field("has_reason", &true)
            .finish()
    }
}

impl SafetyOverride {
    pub(crate) fn new(protection: &'static str, reason: Arc<str>) -> Self {
        Self { protection, reason }
    }

    /// Returns the name of the baseline protection that was explicitly removed.
    pub const fn protection(&self) -> &'static str {
        self.protection
    }
    /// Returns the application-recorded reason for removing the protection.
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

/// Immutable configuration snapshot shared by endpoint clones.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveConfiguration {
    pub(crate) execution_limits: SettingSnapshot<ExecutionLimits>,
    pub(crate) document_cache: SettingSnapshot<DocumentCacheLimits>,
    pub(crate) introspection_enabled: SettingSnapshot<bool>,
    #[cfg(feature = "graphiql")]
    pub(crate) graphiql_enabled: SettingSnapshot<bool>,
    pub(crate) body_limit: SettingSnapshot<usize>,
    pub(crate) response_byte_limit: SettingSnapshot<usize>,
    pub(crate) body_timeout: SettingSnapshot<Duration>,
    pub(crate) transport_admission_timeout: SettingSnapshot<Duration>,
    pub(crate) source_resolution_timeout: SettingSnapshot<Duration>,
    pub(crate) decoded_request_timeout: SettingSnapshot<Duration>,
    pub(crate) context_timeout: SettingSnapshot<Duration>,
    pub(crate) execution_timeout: SettingSnapshot<Duration>,
    pub(crate) response_hook_timeout: SettingSnapshot<Duration>,
    pub(crate) overall_timeout: SettingSnapshot<Duration>,
    pub(crate) subscription_limits: SettingSnapshot<SubscriptionLimits>,
    pub(crate) subscription_protocol: SettingSnapshot<SubscriptionProtocolSettings>,
    pub(crate) connection_init_authorization: SettingSnapshot<ConnectionInitAuthorization>,
    pub(crate) rate_coverage: RateLimitCoverage,
    pub(crate) stage_a_rate_limit: Option<RateLimitStageConfiguration>,
    pub(crate) stage_b_rate_limit: Option<RateLimitStageConfiguration>,
    pub(crate) safety_overrides: Vec<SafetyOverride>,
}

impl EffectiveConfiguration {
    /// Returns baseline and final execution limits with their source.
    pub const fn execution_limits(&self) -> &SettingSnapshot<ExecutionLimits> {
        &self.execution_limits
    }
    /// Returns document-cache limits.
    pub const fn document_cache(&self) -> &SettingSnapshot<DocumentCacheLimits> {
        &self.document_cache
    }
    /// Returns whether introspection is enabled.
    pub const fn introspection_enabled(&self) -> &SettingSnapshot<bool> {
        &self.introspection_enabled
    }
    /// Returns whether GraphiQL is enabled; enabling it also exposes introspection.
    #[cfg(feature = "graphiql")]
    pub const fn graphiql_enabled(&self) -> &SettingSnapshot<bool> {
        &self.graphiql_enabled
    }
    /// Returns the request-body maximum-byte snapshot.
    pub const fn body_limit(&self) -> &SettingSnapshot<usize> {
        &self.body_limit
    }
    /// Returns the response maximum-byte snapshot.
    pub const fn response_byte_limit(&self) -> &SettingSnapshot<usize> {
        &self.response_byte_limit
    }
    /// Returns the body-read deadline snapshot.
    pub const fn body_timeout(&self) -> &SettingSnapshot<Duration> {
        &self.body_timeout
    }
    /// Returns the transport-admission deadline.
    pub const fn transport_admission_timeout(&self) -> &SettingSnapshot<Duration> {
        &self.transport_admission_timeout
    }
    /// Returns the snapshot of the deadline for operation source resolution.
    pub const fn source_resolution_timeout(&self) -> &SettingSnapshot<Duration> {
        &self.source_resolution_timeout
    }
    /// Returns the decoded-request hook deadline.
    pub const fn decoded_request_timeout(&self) -> &SettingSnapshot<Duration> {
        &self.decoded_request_timeout
    }
    /// Returns the context-factory deadline.
    pub const fn context_timeout(&self) -> &SettingSnapshot<Duration> {
        &self.context_timeout
    }
    /// Return the GraphQL execution deadline snapshot.
    pub const fn execution_timeout(&self) -> &SettingSnapshot<Duration> {
        &self.execution_timeout
    }
    /// Returns the response-hook deadline.
    pub const fn response_hook_timeout(&self) -> &SettingSnapshot<Duration> {
        &self.response_hook_timeout
    }
    /// Snapshot of the overall HTTP request deadline.
    pub const fn overall_timeout(&self) -> &SettingSnapshot<Duration> {
        &self.overall_timeout
    }
    /// Returns the complete subscription delivery limits setting.
    pub const fn subscription_limits(&self) -> &SettingSnapshot<SubscriptionLimits> {
        &self.subscription_limits
    }
    /// Returns the complete WebSocket subscription protocol setting.
    pub const fn subscription_protocol(&self) -> &SettingSnapshot<SubscriptionProtocolSettings> {
        &self.subscription_protocol
    }
    /// Describes the connection-initialization callback without exposing it.
    pub const fn connection_init_authorization(
        &self,
    ) -> &SettingSnapshot<ConnectionInitAuthorization> {
        &self.connection_init_authorization
    }
    /// Returns the Stage A/B rate-limit attachment scope.
    pub const fn rate_limit_coverage(&self) -> RateLimitCoverage {
        self.rate_coverage
    }
    /// Returns the final redacted configuration applied to Stage A.
    pub const fn stage_a_rate_limit(&self) -> Option<RateLimitStageConfiguration> {
        self.stage_a_rate_limit
    }
    /// Returns the final redacted configuration applied to Stage B.
    pub const fn stage_b_rate_limit(&self) -> Option<RateLimitStageConfiguration> {
        self.stage_b_rate_limit
    }
    /// Returns records of baseline protections that were explicitly removed.
    pub fn safety_overrides(&self) -> &[SafetyOverride] {
        &self.safety_overrides
    }
}
