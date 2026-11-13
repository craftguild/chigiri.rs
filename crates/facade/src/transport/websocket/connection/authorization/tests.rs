//! A stopped connection cannot enter the application authorizer on first poll.

use super::*;
use crate::subscription::request::MetadataRetentionLimit;
use chigiri_http::{
    ApplicationMetadata, ConnectionInitContext, ConnectionInitError, TransportRequest,
};
use chigiri_runtime::{RequestTelemetry, SamplingDecision};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Authorize(Arc<AtomicUsize>);
impl ConnectionInitAuthorizer for Authorize {
    fn authorize<'a>(
        &'a self,
        _: ConnectionInitContext<'a>,
    ) -> crate::BoxFuture<'a, Result<ConnectionInitDecision, ConnectionInitError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(std::future::ready(Ok(ConnectionInitDecision::Accept)))
    }
}
struct Observe(Arc<AtomicUsize>);
impl AuthorizationErrorObserver for Observe {
    fn observe(&self, _: &(dyn Error + Send + Sync + 'static)) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn stop_before_first_poll_never_calls_the_application_authorizer() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        for options in [
            crate::ShutdownOptions::immediate(),
            crate::ShutdownOptions::graceful(),
        ] {
            let control = crate::managed::Control::new();
            control.activation_started().unwrap();
            let activity = EventActivity::new(Some(Arc::downgrade(&control)));
            let metadata = ApplicationMetadata::default();
            let transport = TransportRequest::new("GET", "/graphql", None, &[], None, &metadata);
            let telemetry = RequestTelemetry::new(None, None, SamplingDecision::Record);
            let request = AdmittedWebSocketRequest::capture(
                transport,
                &telemetry,
                "dGhlIHNhbXBsZSBub25jZQ==",
                MetadataRetentionLimit::new(16 * 1024).unwrap(),
                None,
            )
            .unwrap();
            let calls = Arc::new(AtomicUsize::new(0));
            let errors = Arc::new(AtomicUsize::new(0));
            let work = authorize(
                Arc::new(request),
                None,
                Arc::new(Authorize(calls.clone())),
                Arc::new(Observe(errors.clone())),
                activity,
            );
            control.begin(options, tokio::time::Instant::now());
            assert!(matches!(
                work.await,
                Err(SubscriptionWorkerRuntimeError::EndpointClosed)
            ));
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            assert_eq!(errors.load(Ordering::SeqCst), 0);
        }
    });
}
