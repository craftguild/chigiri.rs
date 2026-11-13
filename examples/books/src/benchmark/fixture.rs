//! Benchmark profile with a real catalog, two-phase rate limiting, observers, and one facade.

use std::sync::Arc;
use std::time::Duration;

use chigiri::rate::memory::{InMemoryRateLimitConfig, InMemoryTokenBucket, SystemClock};
use chigiri::rate::{OperationCost, QuotaKey, QuotaSubject, RatePolicy, TokenBucketPolicy};
use chigiri::{DocumentCacheLimits, Endpoint, OperationContext};
use chigiri_http::{
    AdmissionError, BoxFuture, ContextFactory, PrepareError, PreparedContext, RequestContext,
    TransportRequest,
};
use chigiri_observability::{
    AlwaysRecordSampler, ErrorRecord, ExporterReceiver, LifecycleExporter, LifecycleRecord,
    RandomErrorIdFactory, RandomRequestIdFactory,
};
use chigiri_operation::{
    CatalogEntry, CatalogLimits, CatalogRevision, InMemoryOperationCatalog, PersistedOperationId,
    PersistedOperationResolver,
};
use chigiri_policy::{
    OperationQuotaExtractor, OperationRateLimit, ProviderFailurePolicy, StageARateLimit,
    TimedRateLimiter, TransportQuotaExtractor, UnitFieldCost,
};

use super::measure::{QUERY, TokioDeadline};
use super::model::ProfileDefinition;
use crate::graphql::generated;
use crate::plugin::RequestCountPlugin;
use crate::resolvers::{BookFields, MutationFields, QueryFields, map_catalog_error};
use crate::{AppContext, Catalog};

#[path = "fixture_observer_queue.rs"]
mod observer_queue;
pub(super) use observer_queue::ObserverSampleCounts;
#[path = "fixture_diagnostic.rs"]
mod diagnostic;
pub(super) use diagnostic::{DiagnosticProfile, diagnostic_profile_definitions};

const RATE_CAPACITY: u64 = 1_000_000_000;
pub(super) const OBSERVER_CAPACITY: usize = 65_536;
pub(super) const LIFECYCLE_EVENTS_PER_REQUEST: usize = 29;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Profile {
    Disabled,
    Operation,
    Rate,
    Observer,
    Facade,
}

impl Profile {
    pub(super) const ALL: [Self; 5] = [
        Self::Disabled,
        Self::Operation,
        Self::Rate,
        Self::Observer,
        Self::Facade,
    ];

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Disabled => "policy-disabled",
            Self::Operation => "operation-enabled",
            Self::Rate => "rate-enabled",
            Self::Observer => "observer-enabled",
            Self::Facade => "facade-composition-enabled",
        }
    }
}

pub(super) fn profile_definitions() -> Vec<ProfileDefinition> {
    Profile::ALL
        .into_iter()
        .map(|profile| ProfileDefinition {
            name: profile.name().to_owned(),
            includes: includes(profile)
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            excludes: excludes(profile)
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
        })
        .collect()
}

fn includes(profile: Profile) -> &'static [&'static str] {
    match profile {
        Profile::Disabled => &["neutral endpoint", "document cache"],
        Profile::Operation => &["safelist catalog digest lookup"],
        Profile::Rate => &["Stage A token bucket", "Stage B cost/token bucket"],
        Profile::Observer => &["request/error ID", "sampling", "bounded observer queue"],
        Profile::Facade => &[
            "Endpoint component graph composition",
            "safelist catalog digest lookup",
            "Stage A and Stage B token buckets",
            "request/error ID and bounded observer queue",
        ],
    }
}

fn excludes(profile: Profile) -> &'static [&'static str] {
    match profile {
        Profile::Disabled => &["operation", "rate", "observer"],
        Profile::Operation => &["rate", "observer"],
        Profile::Rate => &["operation source", "observer"],
        Profile::Observer => &["operation source", "rate"],
        Profile::Facade => &["external network provider", "backend exporter worker"],
    }
}

pub(super) struct Fixture {
    pub(super) endpoint: Endpoint<AppContext>,
    pub(super) context: AppContext,
    profile_name: &'static str,
    lifecycle_receiver: Option<ExporterReceiver<LifecycleRecord>>,
    error_receiver: Option<ExporterReceiver<ErrorRecord>>,
}

impl Fixture {
    pub(super) fn new(
        profile: Profile,
        cache_enabled: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let catalog = Arc::new(Catalog::seeded());
        let (endpoint, receivers) = match profile {
            Profile::Disabled => (builder(cache_enabled, Arc::clone(&catalog)).build()?, None),
            Profile::Operation => (
                builder(cache_enabled, Arc::clone(&catalog))
                    .operation_source(operation_source()?)
                    .build()?,
                None,
            ),
            Profile::Rate => rate_fixture(cache_enabled, Arc::clone(&catalog))?,
            Profile::Observer => observer_fixture(cache_enabled, Arc::clone(&catalog))?,
            Profile::Facade => facade_fixture(cache_enabled, Arc::clone(&catalog))?,
        };
        Ok(Self::from_parts(
            profile.name(),
            catalog,
            endpoint,
            receivers,
        ))
    }

    fn from_parts(
        profile_name: &'static str,
        catalog: Arc<Catalog>,
        endpoint: Endpoint<AppContext>,
        receivers: Option<(
            ExporterReceiver<LifecycleRecord>,
            ExporterReceiver<ErrorRecord>,
        )>,
    ) -> Self {
        let (lifecycle_receiver, error_receiver) = receivers
            .map(|(lifecycle, errors)| (Some(lifecycle), Some(errors)))
            .unwrap_or((None, None));
        Self {
            endpoint,
            context: AppContext { catalog },
            profile_name,
            lifecycle_receiver,
            error_receiver,
        }
    }
}

type Builder = chigiri::EndpointBuilder<AppContext, (), Factory>;
type Receivers = Option<(
    ExporterReceiver<LifecycleRecord>,
    ExporterReceiver<ErrorRecord>,
)>;

fn builder(cache_enabled: bool, catalog: Arc<Catalog>) -> Builder {
    let resolvers = generated::Resolvers::builder(map_catalog_error)
        .with_query(QueryFields)
        .with_mutation(MutationFields)
        .with_book(BookFields)
        .build();
    let builder = Endpoint::builder(resolvers)
        .context_factory(Factory(catalog))
        .push_plugin(RequestCountPlugin);
    if cache_enabled {
        builder
    } else {
        builder.document_cache(DocumentCacheLimits::disabled())
    }
}

fn rate_fixture(
    cache_enabled: bool,
    catalog: Arc<Catalog>,
) -> Result<(Endpoint<AppContext>, Receivers), Box<dyn std::error::Error>> {
    let (stage_a, stage_b) = rate_stages()?;
    let endpoint = builder(cache_enabled, catalog)
        .inject(|components| {
            components
                .rate_limit()
                .stage_a_provider(stage_a)
                .stage_b_provider(stage_b);
        })
        .build()?;
    Ok((endpoint, None))
}

fn observer_fixture(
    cache_enabled: bool,
    catalog: Arc<Catalog>,
) -> Result<(Endpoint<AppContext>, Receivers), Box<dyn std::error::Error>> {
    let (observer, lifecycle, errors) = LifecycleExporter::new(OBSERVER_CAPACITY)?;
    let request_ids = RandomRequestIdFactory::new()?;
    let endpoint = builder(cache_enabled, catalog)
        .inject(|components| {
            components
                .observability()
                .error_id_factory(RandomErrorIdFactory)
                .request_id_factory(request_ids)
                .sampler(AlwaysRecordSampler)
                .lifecycle_observer(observer.clone())
                .error_observer(observer);
        })
        .build()?;
    Ok((endpoint, Some((lifecycle, errors))))
}

fn facade_fixture(
    cache_enabled: bool,
    catalog: Arc<Catalog>,
) -> Result<(Endpoint<AppContext>, Receivers), Box<dyn std::error::Error>> {
    let (observer, lifecycle, errors) = LifecycleExporter::new(OBSERVER_CAPACITY)?;
    let (stage_a, stage_b) = rate_stages()?;
    let request_ids = RandomRequestIdFactory::new()?;
    let endpoint = builder(cache_enabled, catalog)
        .operation_source(operation_source()?)
        .inject(|components| {
            components
                .rate_limit()
                .stage_a_provider(stage_a)
                .stage_b_provider(stage_b);
            components
                .observability()
                .error_id_factory(RandomErrorIdFactory)
                .request_id_factory(request_ids)
                .sampler(AlwaysRecordSampler)
                .lifecycle_observer(observer.clone())
                .error_observer(observer);
        })
        .build()?;
    Ok((endpoint, Some((lifecycle, errors))))
}

type StageB = OperationRateLimit<FixedOperationQuota>;
type StageA = StageARateLimit<FixedTransportQuota>;

fn rate_stages() -> Result<(StageA, StageB), Box<dyn std::error::Error>> {
    let policy = rate_policy()?;
    let stage_b = OperationRateLimit::new(
        FixedOperationQuota,
        UnitFieldCost,
        TimedRateLimiter::new(rate_limiter()?, TokioDeadline, Duration::from_secs(1))?,
        policy,
        ProviderFailurePolicy::FailClosed,
        OperationCost::new(RATE_CAPACITY)?,
    )?;
    let stage_a = StageARateLimit::new(
        FixedTransportQuota,
        TimedRateLimiter::new(rate_limiter()?, TokioDeadline, Duration::from_secs(1))?,
        policy,
        ProviderFailurePolicy::FailClosed,
    );
    Ok((stage_a, stage_b))
}

fn operation_source() -> Result<PersistedOperationResolver, Box<dyn std::error::Error>> {
    let entry = CatalogEntry::new(PersistedOperationId::new("benchmark-query")?, QUERY);
    let catalog = InMemoryOperationCatalog::new(
        CatalogRevision::new("benchmark-v2")?,
        vec![entry],
        CatalogLimits::default(),
    )?;
    Ok(PersistedOperationResolver::safelist(catalog))
}

fn rate_policy() -> Result<RatePolicy, Box<dyn std::error::Error>> {
    Ok(TokenBucketPolicy::new(RATE_CAPACITY, RATE_CAPACITY, Duration::from_secs(1))?.into())
}

fn rate_limiter() -> Result<InMemoryTokenBucket<SystemClock>, Box<dyn std::error::Error>> {
    let config = InMemoryRateLimitConfig::new(1, Duration::from_secs(60))?;
    Ok(InMemoryTokenBucket::with_config(config))
}

struct Factory(Arc<Catalog>);

impl ContextFactory<AppContext, ()> for Factory {
    fn create<'a>(
        &'a self,
        _: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<AppContext, ()>, PrepareError>> {
        let catalog = Arc::clone(&self.0);
        Box::pin(async move { Ok(PreparedContext::execution_only(AppContext { catalog })) })
    }
}

struct FixedTransportQuota;

impl TransportQuotaExtractor for FixedTransportQuota {
    fn extract(&self, _: TransportRequest<'_>) -> Result<QuotaKey, AdmissionError> {
        quota_key()
    }
}

struct FixedOperationQuota;

impl OperationQuotaExtractor<AppContext> for FixedOperationQuota {
    fn extract(
        &self,
        _: &OperationContext<'_, AppContext>,
    ) -> Result<QuotaKey, chigiri::GraphQlError> {
        quota_key().map_err(|_| {
            chigiri::GraphQlError::new("Invalid benchmark quota key").code("INTERNAL_SERVER_ERROR")
        })
    }
}

fn quota_key() -> Result<QuotaKey, AdmissionError> {
    QuotaKey::new(b"benchmark".to_vec(), QuotaSubject::Missing, None).map_err(AdmissionError::new)
}
