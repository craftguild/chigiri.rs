//! Fresh streaming phases and explicit finite-scope handoff share one factory.

use std::sync::Arc;

use chigiri::{BoxFuture, ContextFactory, HttpRequestContext, PrepareError, PreparedContext};

use crate::service::{Client, HttpScope, HttpScopes, hooks};

pub(super) struct Scopes {
    pub(super) create: Arc<dyn Fn() -> HttpScope + Send + Sync>,
    pub(super) http: Option<Arc<HttpScopes>>,
}

pub(super) struct Factory(pub(super) Client, pub(super) Option<Scopes>);

impl ContextFactory<Client, ()> for Factory {
    fn create<'a>(
        &'a self,
        request: HttpRequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<Client, ()>, PrepareError>> {
        Box::pin(async move {
            if request.phase() == chigiri::RequestPhase::Finite {
                if let Some(http) = self.1.as_ref().and_then(|scopes| scopes.http.as_ref()) {
                    let scope = http.scope(request.transport().trace_id()).ok_or_else(|| {
                        chigiri::HookError::new(crate::luau::ScriptError::new(
                            "environment",
                            "managed HTTP scope is unavailable",
                        ))
                    })?;
                    return Ok(hooks::prepared_scoped(self.0.clone(), request, scope));
                }
                return Ok(hooks::prepared(self.0.clone(), request));
            }
            let scope = self
                .1
                .as_ref()
                .map(|scopes| (scopes.create)())
                .unwrap_or_else(HttpScope::fresh_from_current);
            Ok(hooks::prepared_scoped(self.0.clone(), request, scope))
        })
    }
}
