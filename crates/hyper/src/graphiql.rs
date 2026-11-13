//! Application-owned Hyper route helper limited to configured paths.

use std::{convert::Infallible, sync::Arc};

use bytes::Bytes;
use chigiri::{BoxFuture, Endpoint, graphiql::Paths};
use http_body_util::Full;
use hyper::{Request, Response, StatusCode, body::Incoming, service::Service};

/// Cheaply cloned service that routes GraphQL and the optional embedded UI.
pub struct GraphiqlService<C> {
    endpoint: Endpoint<C>,
    paths: Arc<Paths>,
}

impl<C> Clone for GraphiqlService<C> {
    fn clone(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            paths: Arc::clone(&self.paths),
        }
    }
}

/// Creates a service for the endpoint's GraphQL path and optional embedded UI.
///
/// The UI and its assets are served only when `.graphiql(true)` is enabled;
/// that setting also exposes schema introspection. The application owns the
/// listener, authentication, CORS, and shutdown. Apply authentication around the
/// whole service and configure any public prefix with [`Paths::with_prefix`].
pub fn service<C>(endpoint: Endpoint<C>, paths: Paths) -> GraphiqlService<C> {
    GraphiqlService {
        endpoint,
        paths: Arc::new(paths),
    }
}

impl<C> Service<Request<Incoming>> for GraphiqlService<C>
where
    C: Send + Sync + 'static,
{
    type Response = Response<Full<Bytes>>;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn call(&self, request: Request<Incoming>) -> Self::Future {
        let endpoint = self.endpoint.clone();
        let paths = Arc::clone(&self.paths);
        Box::pin(async move {
            if request.uri().path() == paths.graphql() {
                return crate::handle(endpoint, request).await;
            }
            if let Some(view) = endpoint.graphiql() {
                // UI requests are routed by fixed paths without polling the body.
                return crate::response::into_response(view.respond(
                    &paths,
                    request.method().as_str(),
                    request.uri().path(),
                ));
            }
            let mut response = Response::new(Full::new(Bytes::new()));
            *response.status_mut() = StatusCode::NOT_FOUND;
            Ok(response)
        })
    }
}
