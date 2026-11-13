//! Create one independent hook/state/budget owner for each execution phase.

use std::sync::{Arc, Mutex, OnceLock};

use super::super::{HttpScope, http::Trace};
use super::Session;
use crate::service::Client;

impl Session {
    pub(in crate::service::hooks) fn new(client: &Client) -> Arc<Self> {
        Self::with_http(client, false)
    }

    pub(in crate::service::hooks) fn with_http(client: &Client, http: bool) -> Arc<Self> {
        let trace = Trace::current().unwrap_or_else(|| Trace::new(None, Arc::new(|_| {})));
        Self::with_trace(client, http, trace, None)
    }

    pub(in crate::service::hooks) fn with_scope(
        client: &Client,
        initial_http: bool,
        scope: HttpScope,
    ) -> Arc<Self> {
        Self::with_trace(client, initial_http, scope.0.clone(), Some(scope))
    }

    fn with_trace(
        client: &Client,
        http: bool,
        trace: Arc<Trace>,
        scope: Option<HttpScope>,
    ) -> Arc<Self> {
        trace.execution_budget.set_clock(&client.clock);
        trace
            .budget
            .lock()
            .expect("observation budget lock")
            .work
            .guard(&trace.cancel, &client.shutdown, trace.deadline);
        // Request metadata is available in streaming phases even though only
        // an initial HTTP response owns a response-completion callback.
        let request = (http || scope.is_some()) && trace.bound.is_some();
        Arc::new(Self {
            trace,
            state: Mutex::new(client.initial_state()),
            operation: OnceLock::new(),
            client: OnceLock::new(),
            source_cancel: OnceLock::new(),
            started: std::sync::atomic::AtomicBool::new(false),
            failure: Mutex::new(None),
            http,
            request,
            _scope: scope,
        })
    }
}
