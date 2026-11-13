//! Bridges Axum requests, bodies, timers, and responses to the transport-neutral endpoint.

mod body;
mod deadline;
mod managed;
mod response;
mod streaming;
mod websocket;

pub use managed::{AxumService, service};

#[cfg(feature = "graphiql")]
pub mod graphiql;

#[doc(hidden)]
pub use websocket::{AxumWebSocketSocket, websocket_socket};

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Request};
use axum::response::Response;
use chigiri::Endpoint;
use chigiri_http::{ApplicationMetadata, BoxFuture, RequestHeader, TraceId, TransportRequest};

/// Converts the neutral endpoint into a handler closure that can be passed
/// directly to the Axum Router.
pub fn handler<C>(
    endpoint: Endpoint<C>,
) -> impl Fn(Request) -> BoxFuture<'static, Response> + Clone + Send + Sync + 'static
where
    C: Send + Sync + 'static,
{
    move |request| {
        let endpoint = endpoint.clone();
        Box::pin(async move { handle(endpoint, request).await })
    }
}

async fn handle<C>(endpoint: Endpoint<C>, request: Request) -> Response
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
    let deadlines = deadline::TokioDeadline;
    let response = endpoint
        .handle(transport, body::AxumBody(body), &deadlines)
        .await;
    response::into_response(response)
}
