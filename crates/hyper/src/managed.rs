//! Native Hyper conversion for the same SDK-managed GraphQL owner as Axum.

use std::convert::Infallible;

use chigiri::__streaming::{ManagedHttpRequest, ManagedHttpResponse, ManagedService};
use chigiri::{Endpoint, ManagedServiceError};
use chigiri_http::{ApplicationMetadata, BoxFuture, RequestHeader, TraceId, TransportRequest};
use hyper::{Request, Response, StatusCode, body::Incoming, service::Service};

use crate::{HyperStreamingBody, PeerAddr, body::HyperBody, deadline::TokioDeadline};

static DEADLINES: TokioDeadline = TokioDeadline;

/// SDK-managed Hyper service sharing supervision and quotas across clones.
///
/// The application keeps its native listener and connection ownership. Last
/// service-owner release requests cancellation, without coordinated server drain.
pub struct ManagedHyperService<C> {
    endpoint: Endpoint<C>,
    managed: ManagedService,
}

impl<C> Clone for ManagedHyperService<C> {
    fn clone(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            managed: self.managed.clone(),
        }
    }
}

/// Activates common SDK supervision fallibly in an existing Tokio runtime.
///
/// The finite-only [`crate::service`] remains available for existing consumers.
pub fn managed_service<C: Send + Sync + 'static>(
    endpoint: Endpoint<C>,
) -> Result<ManagedHyperService<C>, ManagedServiceError> {
    let managed = endpoint.__managed_service()?;
    Ok(ManagedHyperService { endpoint, managed })
}

impl<C: Send + Sync + 'static> Service<Request<Incoming>> for ManagedHyperService<C> {
    type Response = Response<HyperStreamingBody>;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Infallible>>;

    fn call(&self, request: Request<Incoming>) -> Self::Future {
        let scope = self.managed.request();
        let endpoint = self.endpoint.clone();
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
    scope: ManagedHttpRequest,
    mut request: Request<Incoming>,
) -> BoxFuture<'static, Response<HyperStreamingBody>> {
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
    let peer = parts.extensions.get::<PeerAddr>().map(|peer| peer.0);
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
        endpoint.__handle_managed_http(scope, transport, HyperBody(body), version, &DEADLINES);
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
) -> Response<HyperStreamingBody> {
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

fn response_into_native(response: ManagedHttpResponse) -> Response<HyperStreamingBody> {
    match response {
        ManagedHttpResponse::Upgrade(_) => {
            response_into_native(ManagedHttpResponse::internal_failure())
        }
        ManagedHttpResponse::Finite(response) => crate::streaming::response::finite(response),
        ManagedHttpResponse::Prepared(response) => crate::streaming::response::prepared(response),
        ManagedHttpResponse::Streaming(response) => crate::streaming::response::streaming(response),
        ManagedHttpResponse::Empty { status } => {
            let mut response = Response::new(HyperStreamingBody::finite(Vec::new()));
            *response.status_mut() =
                StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            response
        }
    }
}
