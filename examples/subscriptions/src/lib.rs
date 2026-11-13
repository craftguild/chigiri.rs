#![forbid(unsafe_code)]

pub mod domain;
mod resolvers;
#[cfg(test)]
mod tests;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use chigiri::{Endpoint, EndpointBuildError, ManagedServiceError, MissingContextEndpointBuilder};

use domain::State;
use resolvers::{Fields, Types, map_error};

pub const ALLOWED_ORIGIN: &str = "http://localhost:3000";

pub fn endpoint(state: State) -> Result<Endpoint<()>, EndpointBuildError> {
    builder(state).without_context().build()
}

fn builder(state: State) -> MissingContextEndpointBuilder<()> {
    let fields = Fields(state);
    let resolvers = generated::Resolvers::<(), Types>::builder(map_error)
        .with_query(fields.clone())
        .with_subscription(fields.clone())
        .with_reading(fields)
        .build();
    Endpoint::builder(resolvers)
}

pub fn router(graphql: Endpoint<()>) -> Result<axum::Router, ManagedServiceError> {
    Ok(axum::Router::new()
        .route_service("/graphql", chigiri_axum::service(graphql)?)
        .route_layer(axum::middleware::from_fn(origin))
        .route("/health", axum::routing::get(|| async { "healthy" })))
}

async fn origin(request: Request<Body>, next: Next) -> Response {
    let mut origins = request.headers().get_all(header::ORIGIN).iter();
    if let Some(origin) = origins.next()
        && (origin.as_bytes() != ALLOWED_ORIGIN.as_bytes() || origins.next().is_some())
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    next.run(request).await
}
