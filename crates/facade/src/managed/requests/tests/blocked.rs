//! Uncooperative synchronous Rust still retains the original concrete owner.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use super::support::invoke_body;
use super::support::{Harness, finite, invoke, joined, runtime, until};
use crate::{ShutdownOptions, ShutdownOutcome};

pub(super) struct Block {
    released: Mutex<bool>,
    changed: Condvar,
}

pub(super) struct Release(Arc<Block>);

impl Block {
    pub(super) fn new() -> (Arc<Self>, Release) {
        let block = Arc::new(Self {
            released: Mutex::new(false),
            changed: Condvar::new(),
        });
        (Arc::clone(&block), Release(block))
    }
    pub(super) fn wait(&self) {
        let mut released = self.released.lock().unwrap();
        while !*released {
            released = self.changed.wait(released).unwrap();
        }
    }
}

impl Drop for Release {
    fn drop(&mut self) {
        *self.0.released.lock().unwrap() = true;
        self.0.changed.notify_all();
    }
}

#[test]
fn context_method_timeout_is_independent_and_retains_count_until_actual_join() {
    let runtime = runtime();
    let (block, release) = Block::new();
    runtime.block_on(async {
        let harness =
            Harness::with_phase_timeout(Duration::from_secs(2), Duration::from_millis(30));
        *harness.probe.block_context.lock().unwrap() = Some(block);
        let service = harness.endpoint.__managed_service().unwrap();
        let response = invoke(&harness.endpoint, &service, "{ ping }");
        until(|| harness.probe.entered.load(Ordering::SeqCst) == 1).await;
        let response = tokio::time::timeout(Duration::from_secs(1), response)
            .await
            .expect("SDK phase timer is independent of blocked application method");
        assert_eq!(finite(response).status, 504);
        assert!(
            !harness.endpoint.managed_control().requests_idle(),
            "timeout response does not prove physical completion"
        );
        assert_eq!(harness.probe.context_drops.load(Ordering::SeqCst), 0);
        drop(release);
        until(|| harness.endpoint.managed_control().requests_idle()).await;
        assert_eq!(harness.probe.context_drops.load(Ordering::SeqCst), 1);
        assert_eq!(harness.probe.resolved.load(Ordering::SeqCst), 0);
        assert_eq!(harness.endpoint.shutdown_outcome(), None);
        drop(service);
        joined(&harness.endpoint).await;
    });
}

#[test]
fn expired_shutdown_transfers_http_join_while_endpoint_is_still_alive() {
    let runtime = runtime();
    let (block, release) = Block::new();
    runtime.block_on(async {
        let harness = Harness::new(Duration::from_secs(2));
        *harness.probe.block_context.lock().unwrap() = Some(block);
        let service = harness.endpoint.__managed_service().unwrap();
        let response = invoke(&harness.endpoint, &service, "mutation { ping }");
        until(|| harness.probe.entered.load(Ordering::SeqCst) == 1).await;
        tokio::time::timeout(
            Duration::from_millis(500),
            harness.endpoint.shutdown_on(
                async {},
                ShutdownOptions::graceful_for(Duration::from_millis(20)).unwrap(),
            ),
        )
        .await
        .unwrap();
        assert_eq!(
            harness.endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::Expired)
        );
        joined(&harness.endpoint).await;
        assert!(!harness.endpoint.managed_control().requests_idle());
        assert!(matches!(
            response.await,
            crate::managed::ManagedHttpResponse::Empty { status: 503 }
        ));
        drop(release);
        until(|| harness.endpoint.managed_control().requests_idle()).await;
        assert_eq!(
            harness.endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::Expired)
        );
        assert_eq!(harness.probe.context_drops.load(Ordering::SeqCst), 1);
        assert_eq!(harness.probe.resolved.load(Ordering::SeqCst), 0);
        let weak = Arc::downgrade(&harness.probe);
        drop(service);
        drop(harness);
        assert!(weak.upgrade().is_none());
    });
}

struct BlockingBody {
    chunk: Option<Vec<u8>>,
    entered: Arc<std::sync::atomic::AtomicBool>,
    block: Arc<Block>,
}

impl chigiri_http::RequestBody for BlockingBody {
    type Chunk = Vec<u8>;
    fn next_chunk(
        &mut self,
    ) -> crate::BoxFuture<'_, Result<Option<Self::Chunk>, chigiri_http::BodyReadError>> {
        let chunk = self.chunk.take();
        Box::pin(async move { Ok(chunk) })
    }
}

impl Drop for BlockingBody {
    fn drop(&mut self) {
        self.entered.store(true, Ordering::SeqCst);
        self.block.wait();
    }
}

#[test]
fn encoded_finite_response_does_not_report_drained_before_blocked_body_disposal() {
    let runtime = runtime();
    let (block, release) = Block::new();
    runtime.block_on(async {
        let harness = Harness::new(Duration::from_secs(2));
        harness.probe.release();
        let service = harness.endpoint.__managed_service().unwrap();
        let entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let response = invoke_body(
            &harness.endpoint,
            &service,
            BlockingBody {
                chunk: Some(br#"{"query":"mutation { ping }"}"#.to_vec()),
                entered: Arc::clone(&entered),
                block,
            },
        );
        until(|| entered.load(Ordering::SeqCst)).await;
        assert_eq!(harness.probe.resolved.load(Ordering::SeqCst), 1);
        assert_eq!(harness.probe.hooks.load(Ordering::SeqCst), 1);
        assert_eq!(
            *harness.probe.completions.lock().unwrap(),
            [chigiri_http::ResponseCompletionOutcome::Encoded]
        );
        assert!(!harness.endpoint.managed_control().requests_idle());
        tokio::time::timeout(
            Duration::from_millis(500),
            harness.endpoint.shutdown_on(
                async {},
                ShutdownOptions::graceful_for(Duration::from_millis(20)).unwrap(),
            ),
        )
        .await
        .unwrap();
        assert_eq!(
            harness.endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::Expired)
        );
        joined(&harness.endpoint).await;
        assert!(!harness.endpoint.managed_control().requests_idle());
        assert!(matches!(
            response.await,
            crate::managed::ManagedHttpResponse::Empty { status: 503 }
        ));
        drop(release);
        until(|| harness.endpoint.managed_control().requests_idle()).await;
        assert_eq!(
            harness.endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::Expired)
        );
        assert_eq!(harness.probe.completions.lock().unwrap().len(), 1);
        let weak = Arc::downgrade(&harness.probe);
        drop(service);
        drop(harness);
        assert!(weak.upgrade().is_none());
    });
}
