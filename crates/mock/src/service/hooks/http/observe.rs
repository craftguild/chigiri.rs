//! Observe finite work in either its ambient or explicitly linked HTTP scope.

use std::sync::{Arc, atomic::Ordering};

use chigiri::{LifecycleEvent, LifecycleObserver, LifecyclePhase, LifecycleResult, TraceId};

use super::{HttpScopes, Trace};

#[derive(Clone)]
pub(in crate::service) struct Observe(Option<Arc<HttpScopes>>);

impl Observe {
    pub(in crate::service) fn new(scopes: Option<Arc<HttpScopes>>) -> Self {
        Self(scopes)
    }

    fn trace(&self, identity: Option<&TraceId>) -> Option<Arc<Trace>> {
        Trace::current().or_else(|| self.0.as_ref()?.trace(identity))
    }
}

impl LifecycleObserver for Observe {
    fn observe(&self, event: LifecycleEvent<'_>) {
        let Some(trace) = self.trace(event.identity().trace_id()) else {
            return;
        };
        if event.phase() == LifecyclePhase::HttpEncode
            && event.result() == LifecycleResult::Succeeded
        {
            trace.encoded.store(true, Ordering::Relaxed);
        }
        if matches!(
            event.result(),
            LifecycleResult::TimedOut | LifecycleResult::Cancelled
        ) {
            trace.failed.store(true, Ordering::Relaxed);
        }
        if matches!(
            event.phase(),
            LifecyclePhase::HttpEncode | LifecyclePhase::ResponseHook
        ) && matches!(
            event.result(),
            LifecycleResult::Failed | LifecycleResult::TimedOut | LifecycleResult::Cancelled
        ) {
            trace.failed.store(true, Ordering::Relaxed);
        }
    }
}

impl chigiri::ErrorObserver for Observe {
    fn observe_error(&self, event: chigiri::ErrorEvent<'_>) {
        let output_limit = event
            .source()
            .is::<chigiri::core::executor::OutputLimitExceeded>();
        // Independent encode/internal failures cannot be masked by hook status.
        if let Some(trace) = self.trace(event.identity().trace_id()) {
            trace.failed.store(true, Ordering::Relaxed);
        }
        if let Ok(session) = crate::service::hooks::session::REQUEST.try_with(Arc::clone)
            && session.started.load(Ordering::Acquire)
        {
            let failure = if output_limit {
                crate::service::hooks::completion::Failure::OutputLimit
            } else {
                crate::service::hooks::completion::Failure::Internal
            };
            session.fail(failure);
        }
    }
}
