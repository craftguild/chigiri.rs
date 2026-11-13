//! Keep CORS, native admission and access logs around the SDK-managed service.

use std::convert::Infallible;

use std::net::SocketAddr;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::{
    Router,
    body::{Body, Bytes, HttpBody},
    extract::ConnectInfo,
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use chigiri::{BoxFuture, Endpoint, ManagedServiceError, graphiql::Paths};
use chigiri_axum::AxumService;
use chigiri_mock::{logging::Logger, service::Client};
use tokio::sync::Semaphore;
use tower::{Service, ServiceExt};

use super::cors::{cors, origin, preflight};
use super::shutdown::Shutdown;

#[derive(Clone)]
pub(super) struct Routing {
    shared: Arc<Shared>,
    managed: AxumService<Client>,
}

struct Shared {
    static_routes: Router,
    origins: Vec<String>,
    admission: Arc<Semaphore>,
    shutdown: Shutdown,
    logger: Logger,
    scopes: super::observations::Scopes,
    http_scopes: Arc<crate::service::HttpScopes>,
}

#[derive(Clone)]
pub(super) struct ConnectionRoutes {
    routing: Routing,
    peer: SocketAddr,
}

impl Routing {
    pub(super) fn new(
        endpoint: Endpoint<Client>,
        origins: Vec<String>,
        shutdown: Shutdown,
        logger: Logger,
        scopes: super::observations::Scopes,
        http_scopes: Arc<crate::service::HttpScopes>,
    ) -> Result<Self, ManagedServiceError> {
        let static_routes = chigiri_axum::graphiql::routes(endpoint.clone(), Paths::default());
        let managed = chigiri_axum::service(endpoint)?;
        Ok(Self {
            shared: Arc::new(Shared {
                static_routes,
                origins,
                admission: Arc::new(Semaphore::new(chigiri_mock::limits::HTTP_REQUESTS)),
                shutdown,
                logger,
                scopes,
                http_scopes,
            }),
            managed,
        })
    }

    pub(super) fn connection(&self, peer: SocketAddr) -> ConnectionRoutes {
        ConnectionRoutes {
            routing: self.clone(),
            peer,
        }
    }
}

impl<B> Service<axum::http::Request<B>> for ConnectionRoutes
where
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Error: Into<axum::BoxError>,
{
    type Response = Response;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Response, Infallible>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: axum::http::Request<B>) -> Self::Future {
        let mut request = request.map(Body::new);
        request.extensions_mut().insert(ConnectInfo(self.peer));
        let shared = self.routing.shared.clone();
        let mut managed = self.routing.managed.clone();
        Box::pin(async move {
            let graphql = request.uri().path() == "/graphql";
            let scope = (shared.scopes)(graphql);
            let head = request.method() == Method::HEAD;
            let access = chigiri_mock::logging::Access::new(
                shared.logger.clone(),
                scope.request_id(),
                request.method().as_str(),
                request.uri().path(),
            );
            let routed = scope.run(async {
                let origin = match origin(&request, &shared.origins) {
                    Ok(origin) => origin,
                    Err(status) => return status.into_response(),
                };
                if !graphql {
                    // The exact GraphQL path is intercepted here; its finite
                    // fallback route inside this router is never reached.
                    let response = shared.static_routes.clone().oneshot(request).await.unwrap();
                    return cors(response, origin);
                }
                if request.method() == Method::OPTIONS {
                    return preflight(&request, origin);
                }
                if shared.shutdown.subscribe().borrow().is_some() {
                    return cors(StatusCode::SERVICE_UNAVAILABLE.into_response(), origin);
                }
                let Ok(permit) = shared.admission.clone().try_acquire_owned() else {
                    return cors(StatusCode::SERVICE_UNAVAILABLE.into_response(), origin);
                };
                if scope.hold_admission(permit).is_err() {
                    return cors(StatusCode::INTERNAL_SERVER_ERROR.into_response(), origin);
                }
                let Ok(identity) = shared.http_scopes.bind(&scope) else {
                    return cors(StatusCode::INTERNAL_SERVER_ERROR.into_response(), origin);
                };
                request.extensions_mut().insert(identity);
                cors(managed.call(request).await.unwrap(), origin)
            });
            let (mut response, native_stop) = tokio::select! {
                response = routed => (response, false),
                _ = shared.shutdown.wait_native() => {
                    let mut response = StatusCode::SERVICE_UNAVAILABLE.into_response();
                    response.headers_mut().insert(header::CONNECTION, "close".parse().unwrap());
                    (response, true)
                }
            };
            if !native_stop && let Some(status) = scope.response_status() {
                *response.status_mut() = StatusCode::from_u16(status).expect("validated status");
            }
            scope.report_not_started(Some(response.status().as_u16()));
            Ok(super::access_body::attach(response, access, head))
        })
    }
}
