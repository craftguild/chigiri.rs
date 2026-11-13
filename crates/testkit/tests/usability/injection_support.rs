//! Fixed operation source used to compare the convenience API with direct HTTP injection.

use chigiri::spi::{
    ApplicationMetadata, BufferedBody, OperationSourceResolver, RequestEnvelope, RequestHeader,
    ResolvedOperationSource, SourceResolutionError,
};

use super::support::{NeverDeadline, block_on};

pub(crate) struct FixedSource;

impl OperationSourceResolver for FixedSource {
    fn resolve<'a>(
        &'a self,
        _: &'a RequestEnvelope,
    ) -> chigiri::BoxFuture<'a, Result<ResolvedOperationSource, SourceResolutionError>> {
        Box::pin(async { Ok(ResolvedOperationSource::new("{ greeting }")) })
    }
}

pub(crate) fn execute_http(endpoint: &chigiri::Endpoint<()>) -> chigiri::ResponseParts {
    let headers = [RequestHeader::new("content-type", b"application/json")];
    let metadata = ApplicationMetadata::default();
    let request =
        chigiri::TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    block_on(endpoint.handle(
        request,
        BufferedBody::new(br#"{"query":"{ ignored }"}"#.to_vec()),
        &NeverDeadline,
    ))
}
