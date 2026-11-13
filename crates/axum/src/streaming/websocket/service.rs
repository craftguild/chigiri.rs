//! Test-only pre-101 service for one isolated native connection.

use std::net::SocketAddr;
use std::task::{Context, Poll};

use axum::body::{Body, Bytes, HttpBody};
use axum::extract::ConnectInfo;
use axum::http::{Request, Response, Version};
use chigiri::__streaming::{HttpStreamingError, WebSocketHttpVersion, WebSocketUpgradeIngress};
use chigiri::Endpoint;
use chigiri_http::{ApplicationMetadata, BoxFuture, RequestHeader, TraceId, TransportRequest};
use tower::Service;

use super::driver::UpgradeEntry;
use super::port::Registration;
use super::response;
use crate::deadline::TokioDeadline;

static DEADLINES: TokioDeadline = TokioDeadline;

/// Test-only native upgrade handoff retaining the exact pre-head boundary.
pub struct AxumWebSocketUpgradeService<C> {
    endpoint: Endpoint<C>,
    registration: Registration<UpgradeEntry>,
}

impl<C> AxumWebSocketUpgradeService<C> {
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

impl<C> Clone for AxumWebSocketUpgradeService<C> {
    fn clone(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            registration: self.registration.clone(),
        }
    }
}

impl<C, B> Service<Request<B>> for AxumWebSocketUpgradeService<C>
where
    C: Send + Sync + 'static,
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Error: Into<axum::BoxError>,
{
    type Response = Response<Body>;
    type Error = HttpStreamingError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<B>) -> Self::Future {
        let endpoint = self.endpoint.clone();
        let registration = self.registration.clone();
        Box::pin(async move { handle(endpoint, registration, request).await })
    }
}

async fn handle<C, B>(
    endpoint: Endpoint<C>,
    registration: Registration<UpgradeEntry>,
    mut request: Request<B>,
) -> Result<Response<Body>, HttpStreamingError>
where
    C: Send + Sync + 'static,
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Error: Into<axum::BoxError>,
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
    let peer = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|connect| connect.0);
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
        WebSocketUpgradeIngress::Finite(parts) => Ok(crate::response::into_response(parts)),
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
