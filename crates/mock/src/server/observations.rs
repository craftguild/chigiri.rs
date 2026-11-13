//! Supply fresh phase scopes with shared server diagnostics and listener metadata.

use std::net::SocketAddr;
use std::sync::Arc;

use chigiri_mock::service::HttpScope;

pub(super) type Scopes = Arc<dyn Fn(bool) -> HttpScope + Send + Sync>;

pub(super) fn scopes(
    bound: SocketAddr,
    logger: chigiri_mock::logging::Logger,
    sources: Arc<crate::config::Origins>,
) -> Scopes {
    Arc::new(move |is_graphql| {
        let logs = logger.clone();
        let diagnostics = logger.clone();
        let warnings = logger.clone();
        let terminal = logger.clone();
        let hook_sources = sources.clone();
        let resolver_sources = sources.clone();
        HttpScope::new(bound)
            .with_subscription_logging(logger.clone())
            .with_events(move |mut event| {
                event.module = Some(hook_sources.hook_module(event.slot));
                logs.hook(event);
            })
            .with_diagnostics(move |mut event| {
                event.module = resolver_sources.resolver_module(&event.coordinate);
                diagnostics.resolver(event);
            })
            .with_limit_warnings(move |id, notice| warnings.limit_warning(id, notice))
            .with_terminal(move |event| {
                if is_graphql {
                    terminal.terminal(event);
                }
            })
    })
}
