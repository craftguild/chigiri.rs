//! Graceful discards unselected ingress while retaining original physical work.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use chigiri_http::{BodyReadError, RequestBody};

use super::blocked::Block;
use super::support::{Harness, invoke, invoke_body, joined, runtime, until};
use crate::managed::ManagedHttpResponse;
use crate::{BoxFuture, ShutdownOptions, ShutdownOutcome};

struct Ingress {
    polled: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
    block: Option<Arc<Block>>,
}

impl RequestBody for Ingress {
    type Chunk = Vec<u8>;
    fn next_chunk(&mut self) -> BoxFuture<'_, Result<Option<Vec<u8>>, BodyReadError>> {
        Box::pin(async {
            self.polled.store(1, Ordering::SeqCst);
            std::future::pending().await
        })
    }
}

impl Drop for Ingress {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
        if let Some(block) = &self.block {
            block.wait();
        }
    }
}

#[test]
fn graceful_cancels_unfinished_body_without_waiting_for_its_request_timeout() {
    let runtime = runtime();
    runtime.block_on(async {
        let harness = Harness::new(Duration::from_secs(2));
        let service = harness.endpoint.__managed_service().unwrap();
        let polled = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicUsize::new(0));
        let response = invoke_body(
            &harness.endpoint,
            &service,
            Ingress {
                polled: polled.clone(),
                dropped: dropped.clone(),
                block: None,
            },
        );
        until(|| polled.load(Ordering::SeqCst) == 1).await;
        harness
            .endpoint
            .shutdown_on(
                async {},
                ShutdownOptions::graceful_for(Duration::from_millis(20)).unwrap(),
            )
            .await;
        joined(&harness.endpoint).await;
        until(|| harness.endpoint.managed_control().requests_idle()).await;
        assert!(matches!(
            response.await,
            ManagedHttpResponse::Empty { status: 503 }
        ));
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert_eq!(harness.probe.entered.load(Ordering::SeqCst), 0);
        assert_eq!(harness.probe.resolved.load(Ordering::SeqCst), 0);
        assert!(harness.probe.completions.lock().unwrap().is_empty());
        assert!(harness.endpoint.managed_control().requests_idle());
        assert_eq!(
            harness.endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::Drained)
        );
    });
}

#[test]
fn graceful_cancels_unselected_operation_source_without_starting_a_context() {
    let runtime = runtime();
    runtime.block_on(async {
        let harness = Harness::new(Duration::from_secs(2));
        harness.probe.pending_source.store(true, Ordering::SeqCst);
        let service = harness.endpoint.__managed_service().unwrap();
        let response = invoke(&harness.endpoint, &service, "{ ping }");
        until(|| harness.probe.source_entered.load(Ordering::SeqCst) == 1).await;
        harness
            .endpoint
            .shutdown_on(
                async {},
                ShutdownOptions::graceful_for(Duration::from_millis(20)).unwrap(),
            )
            .await;
        joined(&harness.endpoint).await;
        until(|| harness.endpoint.managed_control().requests_idle()).await;
        assert!(matches!(
            response.await,
            ManagedHttpResponse::Empty { status: 503 }
        ));
        assert_eq!(harness.probe.source_drops.load(Ordering::SeqCst), 1);
        assert_eq!(harness.probe.entered.load(Ordering::SeqCst), 0);
        assert!(harness.probe.completions.lock().unwrap().is_empty());
        assert!(harness.endpoint.managed_control().requests_idle());
        assert_eq!(
            harness.endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::Drained)
        );
    });
}

#[test]
fn stop_accepted_inside_source_return_precedes_finite_context_admission() {
    let runtime = runtime();
    runtime.block_on(async {
        let harness = Harness::new(Duration::from_secs(2));
        let service = harness.endpoint.__managed_service().unwrap();
        *harness.probe.stop_at_source_return.lock().unwrap() =
            Some(harness.endpoint.managed_control());
        let response = invoke(&harness.endpoint, &service, "{ ping }");
        joined(&harness.endpoint).await;
        until(|| harness.endpoint.managed_control().requests_idle()).await;
        assert!(matches!(
            response.await,
            ManagedHttpResponse::Empty { status: 503 }
        ));
        assert_eq!(harness.probe.entered.load(Ordering::SeqCst), 0);
        assert_eq!(harness.probe.resolved.load(Ordering::SeqCst), 0);
        assert!(harness.probe.completions.lock().unwrap().is_empty());
        assert_eq!(
            harness.endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::Drained)
        );
    });
}

#[test]
fn blocked_unselected_body_disposal_keeps_the_original_request_after_expiry() {
    let runtime = runtime();
    let (block, release) = Block::new();
    runtime.block_on(async {
        let harness = Harness::new(Duration::from_secs(2));
        let service = harness.endpoint.__managed_service().unwrap();
        let polled = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicUsize::new(0));
        let response = invoke_body(
            &harness.endpoint,
            &service,
            Ingress {
                polled: polled.clone(),
                dropped: dropped.clone(),
                block: Some(block),
            },
        );
        until(|| polled.load(Ordering::SeqCst) == 1).await;
        let mut hook = harness.endpoint.shutdown_on(
            async {},
            ShutdownOptions::graceful_for(Duration::from_millis(500)).unwrap(),
        );
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(hook.as_mut().poll(cx).is_pending()))
                .await
        );
        until(|| dropped.load(Ordering::SeqCst) == 1).await;
        let cancelled_before_expiry = harness.endpoint.shutdown_outcome().is_none();
        let retained_before_expiry = !harness.endpoint.managed_control().requests_idle();
        hook.await;
        let outcome = harness.endpoint.shutdown_outcome();
        let retained_after_expiry = !harness.endpoint.managed_control().requests_idle();
        drop(release);
        until(|| harness.endpoint.managed_control().requests_idle()).await;
        joined(&harness.endpoint).await;
        assert!(matches!(
            response.await,
            ManagedHttpResponse::Empty { status: 503 }
        ));
        assert!(cancelled_before_expiry);
        assert!(retained_before_expiry && retained_after_expiry);
        assert_eq!(outcome, Some(ShutdownOutcome::Expired));
        assert_eq!(harness.endpoint.shutdown_outcome(), outcome);
        assert_eq!(harness.probe.entered.load(Ordering::SeqCst), 0);
        assert!(harness.probe.errors.lock().unwrap().is_empty());
    });
}
