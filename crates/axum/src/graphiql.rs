//! Mount GraphQL and the optional embedded UI in an application-owned router.

use std::sync::Arc;

use axum::{Router, extract::Request, routing::any};
use chigiri::{Endpoint, graphiql::Paths};

/// Creates routes for the endpoint's GraphQL path and optional embedded UI.
///
/// The UI and its assets are registered only when `.graphiql(true)` is enabled;
/// that setting also exposes schema introspection. The application still owns
/// authentication, CORS, listener setup, and shutdown. Use [`Paths::with_prefix`]
/// before merging the returned router under a public prefix. Authentication
/// middleware applied to this router can protect GraphQL, the UI, and its assets
/// together. The helper performs no implicit nesting.
pub fn routes<C, S>(endpoint: Endpoint<C>, paths: Paths) -> Router<S>
where
    C: Send + Sync + 'static,
    S: Clone + Send + Sync + 'static,
{
    let view = endpoint.graphiql();
    let mut router = Router::new().route(paths.graphql(), any(crate::handler(endpoint)));
    if let Some(view) = view {
        let paths = Arc::new(paths);
        let ui = {
            let paths = Arc::clone(&paths);
            move |request: Request| {
                let paths = Arc::clone(&paths);
                async move {
                    // UI routing ignores the body, Host/Forwarded headers, and query,
                    // and it never executes GraphQL.
                    crate::response::into_response(view.respond(
                        &paths,
                        request.method().as_str(),
                        request.uri().path(),
                    ))
                }
            }
        };
        router = router
            .route(paths.ui(), any(ui.clone()))
            .route(&format!("{}{{*asset}}", paths.assets_prefix()), any(ui));
    }
    router
}
