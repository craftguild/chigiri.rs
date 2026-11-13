//! Native Router service; execution and infrastructure policy stay in the SDK.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::task::{Context, Poll};

use axum::body::{Body, Bytes, HttpBody};
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use chigiri::__streaming::{ManagedHttpResponse, ManagedService};
use chigiri::{Endpoint, ManagedServiceError};
use chigiri_http::{ApplicationMetadata, BoxFuture, RequestHeader, TraceId, TransportRequest};
use tower::Service;

use crate::{body::AxumBody, deadline::TokioDeadline};

static DEADLINES: TokioDeadline = TokioDeadline;

/// SDK-managed service for ordinary [`axum::Router::route_service`].
///
/// Clones share supervision and quotas. The application owns its listener,
/// Router and runtime. Dropping the last service requests SDK cancellation;
/// route registration alone does not coordinate native server shutdown.
pub struct AxumService<C> {
    endpoint: Endpoint<C>,
    managed: ManagedService,
}

impl<C> Clone for AxumService<C> {
    fn clone(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            managed: self.managed.clone(),
        }
    }
}

/// Activates shared SDK supervision in the existing Tokio runtime, fallibly.
pub fn service<C: Send + Sync + 'static>(
    endpoint: Endpoint<C>,
) -> Result<AxumService<C>, ManagedServiceError> {
    let managed = endpoint.__managed_service()?;
    Ok(AxumService { endpoint, managed })
}

impl<C, B> Service<Request<B>> for AxumService<C>
where
    C: Send + Sync + 'static,
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Error: Into<axum::BoxError>,
{
    type Response = Response;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Response, Infallible>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<B>) -> Self::Future {
        let scope = self.managed.request();
        let endpoint = self.endpoint.clone();
        let request = request.map(Body::new);
        match scope {
            Ok(scope) => {
                let response = handle(&endpoint, scope, request);
                Box::pin(async move { Ok(response.await) })
            }
            Err(_) => {
                Box::pin(async { Ok(response_into_native(ManagedHttpResponse::unavailable())) })
            }
        }
    }
}

fn handle<C: Send + Sync + 'static>(
    endpoint: &Endpoint<C>,
    scope: chigiri::__streaming::ManagedHttpRequest,
    mut request: Request<Body>,
) -> BoxFuture<'static, Response> {
    let upgrade = hyper::upgrade::on(&mut request);
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
    let mut transport = TransportRequest::new(
        parts.method.as_str(),
        parts.uri.path(),
        parts.uri.query(),
        &headers,
        peer,
        metadata,
    );
    if let Some(trace) = parts.extensions.get::<TraceId>() {
        transport = transport.with_trace_id(trace);
    }
    let version = if parts.version == hyper::Version::HTTP_11 {
        chigiri::__streaming::WebSocketHttpVersion::Http11
    } else {
        chigiri::__streaming::WebSocketHttpVersion::Unsupported
    };
    let response =
        endpoint.__handle_managed_http(scope, transport, AxumBody(body), version, &DEADLINES);
    let endpoint = endpoint.clone();
    Box::pin(async move {
        match response.await {
            ManagedHttpResponse::Upgrade(seed) => upgraded_response(&endpoint, *seed, upgrade),
            response => response_into_native(response),
        }
    })
}

fn upgraded_response<C: Send + Sync + 'static>(
    endpoint: &Endpoint<C>,
    seed: chigiri::__streaming::ManagedWebSocketUpgrade,
    upgrade: hyper::upgrade::OnUpgrade,
) -> Response {
    let response =
        match crate::streaming::websocket::response::switching_parts(seed.key(), seed.protocol()) {
            Ok(response) => response,
            Err(_) => return response_into_native(ManagedHttpResponse::internal_failure()),
        };
    let socket = Box::pin(async move {
        let upgraded = upgrade
            .await
            .map_err(|_| chigiri::__streaming::WebSocketIoError::Transport)?;
        Ok(crate::websocket::websocket_socket(upgraded).await)
    });
    match endpoint.__attach_managed_websocket(seed, socket, &DEADLINES) {
        Ok(()) => response,
        Err(_) => response_into_native(ManagedHttpResponse::internal_failure()),
    }
}

fn response_into_native(response: ManagedHttpResponse) -> Response {
    match response {
        ManagedHttpResponse::Upgrade(_) => {
            response_into_native(ManagedHttpResponse::internal_failure())
        }
        ManagedHttpResponse::Finite(response) => crate::response::into_response(response),
        ManagedHttpResponse::Prepared(response) => crate::streaming::response::prepared(response),
        ManagedHttpResponse::Streaming(response) => crate::streaming::response::streaming(response),
        ManagedHttpResponse::Empty { status } => {
            let mut response = Response::new(Body::empty());
            *response.status_mut() =
                StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            response
        }
    }
}
