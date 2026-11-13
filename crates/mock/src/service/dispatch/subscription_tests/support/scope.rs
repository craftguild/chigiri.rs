//! Select explicit diagnostic sinks without coupling delivery metadata to logging.

use super::{Arc, AtomicUsize, HttpScope, Mutex, Ordering, ResolverDiagnostic};

pub(super) fn build(
    logger: Option<crate::logging::Logger>,
    resolver_calls: Arc<Mutex<Vec<ResolverDiagnostic>>>,
    hook_calls: Arc<AtomicUsize>,
) -> HttpScope {
    let scope = HttpScope::new("127.0.0.1:49123".parse().unwrap());
    let scope = match logger {
        Some(logger) => scope.with_subscription_logging(logger),
        None => scope,
    };
    scope
        .with_diagnostics(move |diagnostic| resolver_calls.lock().unwrap().push(diagnostic))
        .with_events(move |event| {
            if event.record.is_err() {
                hook_calls.fetch_add(1, Ordering::SeqCst);
            }
        })
}
