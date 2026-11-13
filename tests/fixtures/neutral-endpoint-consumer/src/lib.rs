//! Consumer fixture for the endpoint SPI with no framework dependency.

use std::time::Duration;

use chigiri::{
    BodyReadError, BoxFuture, DeadlineFactory, Endpoint, RequestBody, ResponseParts,
    TransportRequest,
};
use chigiri::spi::{OperationSourceResolver, RequestEnvelope, ResolvedOperationSource};

pub struct ConsumerBody(Option<Vec<u8>>);

impl RequestBody for ConsumerBody {
    type Chunk = Vec<u8>;

    fn next_chunk(&mut self) -> BoxFuture<'_, Result<Option<Self::Chunk>, BodyReadError>> {
        let chunk = self.0.take();
        Box::pin(async move { Ok(chunk) })
    }
}

pub struct ConsumerDeadlines;

impl DeadlineFactory for ConsumerDeadlines {
    fn sleep(&self, _: Duration) -> BoxFuture<'_, ()> {
        Box::pin(std::future::pending())
    }
}

pub struct ConsumerSource;

impl OperationSourceResolver for ConsumerSource {
    fn resolve<'a>(
        &'a self,
        _: &'a RequestEnvelope,
    ) -> BoxFuture<'a, Result<ResolvedOperationSource, chigiri::spi::SourceResolutionError>> {
        Box::pin(async { Ok(ResolvedOperationSource::new("{ __typename }")) })
    }
}

pub struct ConsumerResponseHook;

impl chigiri::ResponseHook<()> for ConsumerResponseHook {
    fn call<'a>(
        &'a self,
        _: chigiri::ResponseHookContext<'a, ()>,
    ) -> BoxFuture<'a, Result<chigiri::ResponseHeaders, chigiri::HookError>> {
        Box::pin(async { Ok(chigiri::ResponseHeaders::new()) })
    }
}

pub fn assert_public_contract(
    request: TransportRequest<'_>,
    body: &mut dyn RequestBody<Chunk = Vec<u8>>,
    deadlines: &dyn DeadlineFactory,
    response: ResponseParts,
) -> u16 {
    let _ = (request, body, deadlines);
    response.status
}

pub fn accept_endpoint<C>(endpoint: Endpoint<C>) -> Endpoint<C> {
    endpoint
}
