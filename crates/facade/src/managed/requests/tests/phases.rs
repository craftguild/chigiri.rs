//! Independent HTTP clocks preserve body/source rejection classifications.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::support::{Harness, finite, invoke, invoke_body, joined, runtime, until};
use crate::BoxFuture;
use chigiri_http::{BodyReadError, RequestBody};

struct PendingBody(Arc<AtomicUsize>);

impl RequestBody for PendingBody {
    type Chunk = Vec<u8>;
    fn next_chunk(&mut self) -> BoxFuture<'_, Result<Option<Vec<u8>>, BodyReadError>> {
        Box::pin(std::future::pending())
    }
}

impl Drop for PendingBody {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn body_timeout_keeps_408_and_the_overall_deadline_keeps_504() {
    let runtime = runtime();
    runtime.block_on(async {
        for (overall, phase, status, code) in [
            (
                Duration::from_secs(2),
                Duration::from_millis(30),
                408,
                "REQUEST_BODY_TIMEOUT",
            ),
            (
                Duration::from_millis(30),
                Duration::from_secs(1),
                504,
                "REQUEST_TIMEOUT",
            ),
        ] {
            let harness = Harness::with_phase_timeout(overall, phase);
            let service = harness.endpoint.__managed_service().unwrap();
            let drops = Arc::new(AtomicUsize::new(0));
            let response =
                invoke_body(&harness.endpoint, &service, PendingBody(Arc::clone(&drops)));
            until(|| harness.endpoint.managed_control().requests_idle()).await;
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            let parts = finite(response.await);
            assert_eq!(parts.status, status);
            assert!(String::from_utf8(parts.body).unwrap().contains(code));
            assert_eq!(harness.probe.entered.load(Ordering::SeqCst), 0);
            assert_eq!(harness.probe.resolved.load(Ordering::SeqCst), 0);
            drop(service);
            joined(&harness.endpoint).await;
        }
    });
}

#[test]
fn operation_source_timeout_keeps_503_before_context_or_execution() {
    let runtime = runtime();
    runtime.block_on(async {
        let harness =
            Harness::with_phase_timeout(Duration::from_secs(2), Duration::from_millis(30));
        harness.probe.pending_source.store(true, Ordering::SeqCst);
        let service = harness.endpoint.__managed_service().unwrap();
        let response = invoke(&harness.endpoint, &service, "{ ping }");
        until(|| harness.endpoint.managed_control().requests_idle()).await;
        assert_eq!(harness.probe.source_drops.load(Ordering::SeqCst), 1);
        let parts = finite(response.await);
        assert_eq!(parts.status, 503);
        assert!(
            String::from_utf8(parts.body)
                .unwrap()
                .contains("PERSISTED_OPERATION_UNAVAILABLE")
        );
        assert_eq!(harness.probe.entered.load(Ordering::SeqCst), 0);
        assert_eq!(harness.probe.resolved.load(Ordering::SeqCst), 0);
        drop(service);
        joined(&harness.endpoint).await;
    });
}
