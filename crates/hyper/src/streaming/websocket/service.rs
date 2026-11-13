//! Test-only pre-101 service for one isolated native connection.

use chigiri::__streaming::{HttpStreamingError, WebSocketHttpVersion, WebSocketUpgradeIngress};
use chigiri::Endpoint;
use chigiri_http::{ApplicationMetadata, BoxFuture, RequestHeader, TraceId, TransportRequest};
use hyper::{Request, Response, Version, body::Incoming, service::Service};

use super::driver::UpgradeEntry;
use super::port::Registration;
use super::response;
use crate::streaming::{HyperStreamingBody, response as http_response};
use crate::{PeerAddr, deadline::TokioDeadline};

static DEADLINES: TokioDeadline = TokioDeadline;

/// Test-only native upgrade handoff retaining the exact pre-head boundary.
pub struct HyperWebSocketUpgradeService<C> {
    endpoint: Endpoint<C>,
    registration: Registration<UpgradeEntry>,
}

impl<C> HyperWebSocketUpgradeService<C> {
    pub(super) const fn new(
        endpoint: Endpoint<C>,
        registration: Registration<UpgradeEntry>,
    ) -> Self {
        Self {
            endpoint,
            registration,
        }
    }
}

impl<C> Clone for HyperWebSocketUpgradeService<C> {
    fn clone(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            registration: self.registration.clone(),
        }
    }
}

impl<C> Service<Request<Incoming>> for HyperWebSocketUpgradeService<C>
where
    C: Send + Sync + 'static,
{
    type Response = Response<HyperStreamingBody>;
    type Error = HttpStreamingError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn call(&self, request: Request<Incoming>) -> Self::Future {
        let endpoint = self.endpoint.clone();
        let registration = self.registration.clone();
        Box::pin(async move { handle(endpoint, registration, request).await })
    }
}

async fn handle<C>(
    endpoint: Endpoint<C>,
    registration: Registration<UpgradeEntry>,
    mut request: Request<Incoming>,
) -> Result<Response<HyperStreamingBody>, HttpStreamingError>
where
    C: Send + Sync + 'static,
{
    let upgrade = hyper::upgrade::on(&mut request);
    let (parts, body) = request.into_parts();
    // Upgrade admission is body-independent. Never poll the request body.
    drop(body);
    let headers: Vec<_> = parts
        .headers
        .iter()
        .map(|(name, value)| RequestHeader::new(name.as_str(), value.as_bytes()))
        .collect();
    let default_metadata = ApplicationMetadata::default();
    let metadata = parts
        .extensions
        .get::<ApplicationMetadata>()
        .unwrap_or(&default_metadata);
    let peer = parts.extensions.get::<PeerAddr>().map(|peer| peer.0);
    let trace_id = parts.extensions.get::<TraceId>();
    let mut transport = TransportRequest::new(
        parts.method.as_str(),
        parts.uri.path(),
        parts.uri.query(),
        &headers,
        peer,
        metadata,
    );
    if let Some(trace_id) = trace_id {
        transport = transport.with_trace_id(trace_id);
    }
    let version = if parts.version == Version::HTTP_11 {
        WebSocketHttpVersion::Http11
    } else {
        WebSocketHttpVersion::Unsupported
    };
    match endpoint
        .__admit_websocket_upgrade(transport, version, &DEADLINES)
        .await
    {
        WebSocketUpgradeIngress::Finite(parts) => Ok(http_response::finite(parts)),
        WebSocketUpgradeIngress::Admitted(seed) => {
            let seed = endpoint
                .__claim_websocket_upgrade(seed)
                .map_err(|_| HttpStreamingError::Invariant)?;
            let response = response::switching(&seed)?;
            registration.attach(UpgradeEntry { seed, upgrade })?;
            Ok(response)
        }
    }
}
