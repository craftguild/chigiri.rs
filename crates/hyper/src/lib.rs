//! Service bridge from Hyper requests, bodies, and timers to the neutral endpoint.

mod body;
mod deadline;
mod managed;
mod response;
mod streaming;
mod websocket;

pub use managed::{ManagedHyperService, managed_service};

#[cfg(feature = "graphiql")]
pub mod graphiql;

pub use streaming::HyperStreamingBody;
#[doc(hidden)]
pub use websocket::{HyperWebSocketSocket, websocket_socket};

use std::convert::Infallible;
use std::net::SocketAddr;

use bytes::Bytes;
use chigiri::Endpoint;
use chigiri_http::{ApplicationMetadata, BoxFuture, RequestHeader, TraceId, TransportRequest};
use http_body_util::Full;
use hyper::{Request, Response, body::Incoming, service::Service};

/// Request extension for direct peer addresses obtained by the application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerAddr(pub SocketAddr);

/// Cheaply cloned Hyper service for an application-owned connection.
pub struct HyperService<C> {
    endpoint: Endpoint<C>,
}

impl<C> Clone for HyperService<C> {
    fn clone(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
        }
    }
}

/// Converts a transport-neutral endpoint into a Hyper service.
pub const fn service<C>(endpoint: Endpoint<C>) -> HyperService<C> {
    HyperService { endpoint }
}

impl<C> Service<Request<Incoming>> for HyperService<C>
where
    C: Send + Sync + 'static,
{
    type Response = Response<Full<Bytes>>;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn call(&self, request: Request<Incoming>) -> Self::Future {
        let endpoint = self.endpoint.clone();
        Box::pin(async move { handle(endpoint, request).await })
    }
}

async fn handle<C>(
    endpoint: Endpoint<C>,
    request: Request<Incoming>,
) -> Result<Response<Full<Bytes>>, Infallible>
where
    C: Send + Sync + 'static,
{
    let (parts, body) = request.into_parts();
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
    let deadlines = deadline::TokioDeadline;
    let response = endpoint
        .handle(transport, body::HyperBody(body), &deadlines)
        .await;
    response::into_response(response)
}
