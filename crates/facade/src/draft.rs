//! Mutable component graph used while building an endpoint.

use std::sync::Arc;
use std::time::Duration;

use chigiri_http::{
    AcceptConnectionInit, ConnectionInitAuthorizer, DecodedRequestHook, OperationSourceResolver,
    TransportAdmission,
};
use chigiri_observability::RandomErrorIdFactory;
use chigiri_runtime::{
    DocumentCacheLimits, ErrorIdFactory, ErrorObserver, ExecutionLimits, LifecycleObserver, Plugin,
    RequestIdFactory, Sampler,
};

use crate::configuration::{
    ConfigurationOrigin, EffectiveConfiguration, RateLimitCoverage, RateLimitStageConfiguration,
    SafetyOverride, SettingSnapshot,
};
use crate::input::EndpointInput;
use crate::subscription::settings::{
    ConnectionInitAuthorization, SubscriptionLimits, SubscriptionProtocolSettings,
};

const DEFAULT_BODY_LIMIT: usize = 1024 * 1024;
const DEFAULT_RESPONSE_LIMIT: usize = 8 * 1024 * 1024;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) struct Slot<T> {
    baseline: T,
    pub(crate) value: T,
    origin: ConfigurationOrigin,
    pub(crate) sequence: u64,
}

impl<T: Clone> Slot<T> {
    pub(crate) fn baseline(value: T) -> Self {
        Self {
            baseline: value.clone(),
            value,
            origin: ConfigurationOrigin::Baseline,
            sequence: 0,
        }
    }

    fn snapshot(&self) -> SettingSnapshot<T> {
        SettingSnapshot::new(self.baseline.clone(), self.value.clone(), self.origin)
    }

    pub(crate) const fn origin(&self) -> ConfigurationOrigin {
        self.origin
    }
}

pub(crate) enum RequestIdPlan {
    Automatic,
    Configured(Arc<dyn RequestIdFactory>),
}

pub(crate) struct Draft<C> {
    pub(crate) input: Option<EndpointInput<C>>,
    pub(crate) sequence: u64,
    pub(crate) limits: Slot<ExecutionLimits>,
    pub(crate) cache: Slot<DocumentCacheLimits>,
    pub(crate) introspection: Slot<bool>,
    #[cfg(feature = "graphiql")]
    pub(crate) graphiql: Slot<bool>,
    pub(crate) body_limit: Slot<usize>,
    pub(crate) response_limit: Slot<usize>,
    pub(crate) body_timeout: Slot<Duration>,
    pub(crate) transport_timeout: Slot<Duration>,
    pub(crate) source_timeout: Slot<Duration>,
    pub(crate) decoded_timeout: Slot<Duration>,
    pub(crate) context_timeout: Slot<Duration>,
    pub(crate) execution_timeout: Slot<Duration>,
    pub(crate) response_hook_timeout: Slot<Duration>,
    pub(crate) overall_timeout: Slot<Duration>,
    pub(crate) subscription_limits: Slot<SubscriptionLimits>,
    pub(crate) subscription_protocol: Slot<SubscriptionProtocolSettings>,
    pub(crate) connection_init_authorizer: Slot<Arc<dyn ConnectionInitAuthorizer>>,
    pub(crate) csrf_enabled: bool,
    pub(crate) source: Option<Arc<dyn OperationSourceResolver>>,
    pub(crate) admissions: Vec<Arc<dyn TransportAdmission>>,
    pub(crate) stage_a: Option<Arc<dyn TransportAdmission>>,
    pub(crate) stage_b: Option<Arc<dyn Plugin<C>>>,
    pub(crate) stage_a_observer: Option<Arc<dyn chigiri_policy::RateLimitObserverBinding>>,
    pub(crate) stage_b_observer: Option<Arc<dyn chigiri_policy::RateLimitObserverBinding>>,
    pub(crate) stage_a_rate_limit: Option<RateLimitStageConfiguration>,
    pub(crate) stage_b_rate_limit: Option<RateLimitStageConfiguration>,
    pub(crate) decoded: Vec<Arc<dyn DecodedRequestHook>>,
    pub(crate) plugins: Vec<Arc<dyn Plugin<C>>>,
    pub(crate) error_ids: Arc<dyn ErrorIdFactory>,
    pub(crate) request_ids: RequestIdPlan,
    pub(crate) sampler: Option<Arc<dyn Sampler>>,
    pub(crate) instrumentation: Option<Arc<dyn chigiri_runtime::instrumentation::Instrumentation>>,
    pub(crate) lifecycle_observer: Option<Arc<dyn LifecycleObserver>>,
    pub(crate) error_observer: Option<Arc<dyn ErrorObserver>>,
    pub(crate) field_instrumentation: bool,
    pub(crate) response_hook: Option<Arc<dyn crate::transport::ErasedResponseHook>>,
    pub(crate) safety_overrides: Vec<SafetyOverride>,
}

impl<C> Draft<C>
where
    C: Send + Sync + 'static,
{
    pub(crate) fn new(input: EndpointInput<C>) -> Self {
        Self {
            input: Some(input),
            sequence: 0,
            limits: Slot::baseline(ExecutionLimits::production()),
            cache: Slot::baseline(DocumentCacheLimits::default()),
            introspection: Slot::baseline(false),
            #[cfg(feature = "graphiql")]
            graphiql: Slot::baseline(false),
            body_limit: Slot::baseline(DEFAULT_BODY_LIMIT),
            response_limit: Slot::baseline(DEFAULT_RESPONSE_LIMIT),
            body_timeout: Slot::baseline(DEFAULT_TIMEOUT),
            transport_timeout: Slot::baseline(DEFAULT_TIMEOUT),
            source_timeout: Slot::baseline(DEFAULT_TIMEOUT),
            decoded_timeout: Slot::baseline(DEFAULT_TIMEOUT),
            context_timeout: Slot::baseline(DEFAULT_TIMEOUT),
            execution_timeout: Slot::baseline(DEFAULT_TIMEOUT),
            response_hook_timeout: Slot::baseline(DEFAULT_TIMEOUT),
            overall_timeout: Slot::baseline(DEFAULT_TIMEOUT),
            subscription_limits: Slot::baseline(SubscriptionLimits::default()),
            subscription_protocol: Slot::baseline(SubscriptionProtocolSettings::default()),
            connection_init_authorizer: Slot::baseline(Arc::new(AcceptConnectionInit)),
            csrf_enabled: true,
            source: None,
            admissions: Vec::new(),
            stage_a: None,
            stage_b: None,
            stage_a_observer: None,
            stage_b_observer: None,
            stage_a_rate_limit: None,
            stage_b_rate_limit: None,
            decoded: Vec::new(),
            plugins: Vec::new(),
            error_ids: Arc::new(RandomErrorIdFactory),
            request_ids: RequestIdPlan::Automatic,
            sampler: None,
            instrumentation: None,
            lifecycle_observer: None,
            error_observer: None,
            field_instrumentation: false,
            response_hook: None,
            safety_overrides: Vec::new(),
        }
    }

    fn next_sequence(&mut self) -> u64 {
        self.sequence = self.sequence.saturating_add(1);
        self.sequence
    }

    pub(crate) fn set<T>(sequence: &mut u64, slot: &mut Slot<T>, value: T) {
        *sequence = sequence.saturating_add(1);
        slot.value = value;
        slot.origin = ConfigurationOrigin::User;
        slot.sequence = *sequence;
    }

    pub(crate) fn mark_change(&mut self) {
        self.next_sequence();
    }

    pub(crate) fn snapshot(&self) -> EffectiveConfiguration {
        let rate_coverage = match (self.stage_a.is_some(), self.stage_b.is_some()) {
            (false, false) => RateLimitCoverage::Unconfigured,
            (true, false) => RateLimitCoverage::Transport,
            (false, true) => RateLimitCoverage::Operation,
            (true, true) => RateLimitCoverage::Both,
        };
        EffectiveConfiguration {
            execution_limits: self.limits.snapshot(),
            document_cache: self.cache.snapshot(),
            introspection_enabled: self.introspection_snapshot(),
            #[cfg(feature = "graphiql")]
            graphiql_enabled: self.graphiql.snapshot(),
            body_limit: self.body_limit.snapshot(),
            response_byte_limit: self.response_limit.snapshot(),
            body_timeout: self.body_timeout.snapshot(),
            transport_admission_timeout: self.transport_timeout.snapshot(),
            source_resolution_timeout: self.source_timeout.snapshot(),
            decoded_request_timeout: self.decoded_timeout.snapshot(),
            context_timeout: self.context_timeout.snapshot(),
            execution_timeout: self.execution_timeout.snapshot(),
            response_hook_timeout: self.response_hook_timeout.snapshot(),
            overall_timeout: self.overall_timeout.snapshot(),
            subscription_limits: self.subscription_limits.snapshot(),
            subscription_protocol: self.subscription_protocol.snapshot(),
            connection_init_authorization: self.connection_init_authorization_snapshot(),
            rate_coverage,
            stage_a_rate_limit: self.stage_a_rate_limit,
            stage_b_rate_limit: self.stage_b_rate_limit,
            safety_overrides: self.final_overrides(),
        }
    }

    pub(crate) fn add_override(&mut self, protection: &'static str, reason: Arc<str>) {
        self.mark_change();
        self.safety_overrides
            .retain(|entry| entry.protection() != protection);
        self.safety_overrides
            .push(SafetyOverride::new(protection, reason));
    }

    pub(crate) fn introspection_enabled(&self) -> bool {
        #[cfg(feature = "graphiql")]
        if self.graphiql.value {
            return true;
        }
        self.introspection.value
    }

    fn introspection_snapshot(&self) -> SettingSnapshot<bool> {
        #[cfg(feature = "graphiql")]
        if self.graphiql.value {
            return SettingSnapshot::new(false, true, ConfigurationOrigin::User);
        }
        self.introspection.snapshot()
    }

    fn connection_init_authorization_snapshot(
        &self,
    ) -> SettingSnapshot<ConnectionInitAuthorization> {
        let effective = match self.connection_init_authorizer.origin() {
            ConfigurationOrigin::Baseline => ConnectionInitAuthorization::DefaultAccept,
            ConfigurationOrigin::User => ConnectionInitAuthorization::ApplicationDefined,
        };
        SettingSnapshot::new(
            ConnectionInitAuthorization::DefaultAccept,
            effective,
            self.connection_init_authorizer.origin(),
        )
    }

    fn final_overrides(&self) -> Vec<SafetyOverride> {
        #[cfg(feature = "graphiql")]
        let extra = self.graphiql.value.then(|| {
            SafetyOverride::new(
                "graphiql-schema-introspection",
                Arc::from("The graphiql is true."),
            )
        });
        #[cfg(not(feature = "graphiql"))]
        let extra = None;
        self.safety_overrides.iter().cloned().chain(extra).collect()
    }
}
