//! External consumer fixture that selects either bridge while preserving default-off behavior.

pub fn endpoint_identity<C>(endpoint: chigiri::Endpoint<C>) -> chigiri::Endpoint<C> {
    endpoint
}

#[cfg(feature = "graphiql")]
pub fn axum_routes(endpoint: chigiri::Endpoint<()>) -> axum::Router {
    let paths = chigiri::graphiql::Paths::default()
        .with_prefix("/api")
        .unwrap();
    chigiri_axum::graphiql::routes(endpoint, paths)
}

#[cfg(feature = "graphiql")]
pub fn hyper_routes(
    endpoint: chigiri::Endpoint<()>,
) -> chigiri_hyper::graphiql::GraphiqlService<()> {
    chigiri_hyper::graphiql::service(endpoint, chigiri::graphiql::Paths::default())
}
