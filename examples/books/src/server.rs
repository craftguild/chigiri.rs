//! Builds the generated schema, middleware, and transport-neutral endpoint.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use chigiri::rate::{OperationQuotaExtractor, QuotaKey, QuotaSubject, TransportQuotaExtractor};
use chigiri::spi::AdmissionError;
use chigiri::{
    BoxFuture, ContextFactory, Endpoint, ErrorEvent, ErrorObserver, HttpRequestContext,
    LifecycleEvent, LifecycleObserver, OperationContext, PrepareError, PreparedContext,
    TransportRequest,
};
use chigiri_observability::{AlwaysRecordSampler, RandomRequestIdFactory};

use crate::Catalog;
use crate::graphql::generated;
use crate::plugin::RequestCountPlugin;
use crate::resolvers::{BookFields, MutationFields, QueryFields, map_catalog_error};

/// The request context shared by the resolver and the plugin.
#[derive(Clone)]
pub struct AppContext {
    pub(crate) catalog: Arc<Catalog>,
}

struct CatalogContextFactory(Arc<Catalog>);

impl ContextFactory<AppContext, ()> for CatalogContextFactory {
    fn create<'a>(
        &'a self,
        _: HttpRequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<AppContext, ()>, PrepareError>> {
        let catalog = Arc::clone(&self.0);
        Box::pin(async move { Ok(PreparedContext::execution_only(AppContext { catalog })) })
    }
}

/// Builds a transport-neutral endpoint with the automatic safe baseline.
pub fn endpoint(catalog: Catalog) -> Result<Endpoint<AppContext>, Box<dyn std::error::Error>> {
    endpoint_with_request_ids(catalog, RandomRequestIdFactory::new())
}

fn endpoint_with_request_ids(
    catalog: Catalog,
    request_ids: Result<RandomRequestIdFactory, chigiri::RequestIdFactoryError>,
) -> Result<Endpoint<AppContext>, Box<dyn std::error::Error>> {
    let request_ids = request_ids?;
    let catalog = Arc::new(catalog);
    let observer = BooksErrorObserver::default();
    let stage_a = chigiri::rate::transport(BooksTransportKey)
        .token_bucket()
        .capacity(1_000_000)?
        .refill(1_000_000, Duration::from_secs(1))?
        .build()?;
    let stage_b = chigiri::rate::operation(BooksOperationKey)
        .token_bucket()
        .capacity(1_000_000)?
        .refill(1_000_000, Duration::from_secs(1))?
        .build()?;
    let resolvers = generated::Resolvers::builder(map_catalog_error)
        .with_query(QueryFields)
        .with_mutation(MutationFields)
        .with_book(BookFields)
        .build();
    Endpoint::builder(resolvers)
        .rate_limit(stage_a)
        .rate_limit(stage_b)
        .context_factory(CatalogContextFactory(catalog))
        .inject(|components| {
            components.plugins().push_plugin(RequestCountPlugin);
            components
                .observability()
                .request_id_factory(request_ids)
                .sampler(AlwaysRecordSampler)
                .lifecycle_observer(BooksLifecycleObserver::default())
                .error_observer(observer);
        })
        .build()
        .map_err(Into::into)
}

struct BooksTransportKey;

impl TransportQuotaExtractor for BooksTransportKey {
    fn extract(&self, _: TransportRequest<'_>) -> Result<QuotaKey, AdmissionError> {
        QuotaKey::new(b"books-http".to_vec(), QuotaSubject::Missing, None)
            .map_err(AdmissionError::new)
    }
}

struct BooksOperationKey;

impl OperationQuotaExtractor<AppContext> for BooksOperationKey {
    fn extract(
        &self,
        _: &OperationContext<'_, AppContext>,
    ) -> Result<QuotaKey, chigiri::GraphQlError> {
        QuotaKey::new(b"books-operation".to_vec(), QuotaSubject::Missing, None).map_err(|_| {
            chigiri::GraphQlError::new("Invalid quota key").code("INTERNAL_SERVER_ERROR")
        })
    }
}

#[derive(Default)]
struct BooksLifecycleObserver(AtomicUsize);

impl LifecycleObserver for BooksLifecycleObserver {
    fn observe(&self, _: LifecycleEvent<'_>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Clone, Default)]
struct BooksErrorObserver(Arc<AtomicUsize>);

impl ErrorObserver for BooksErrorObserver {
    fn observe_error(&self, _: ErrorEvent<'_>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use chigiri::RequestIdFactoryError;

    use super::endpoint_with_request_ids;
    use crate::Catalog;

    #[test]
    fn propagates_request_id_seed_failures() {
        let result = endpoint_with_request_ids(
            Catalog::seeded(),
            Err(RequestIdFactoryError::new(SeedFailure)),
        );
        assert!(result.is_err());
    }

    #[derive(Debug)]
    struct SeedFailure;

    impl std::fmt::Display for SeedFailure {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("seed source failed")
        }
    }

    impl std::error::Error for SeedFailure {}
}
