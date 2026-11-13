//! Managed finite completion is attempted once through cancellation and failure.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::{ShutdownOptions, ShutdownOutcome};
use chigiri_http::ResponseCompletionOutcome;

use super::blocked::Block;
use super::support::{Harness, Mode, finite, invoke, joined, runtime, until};

#[test]
fn late_ready_context_arms_completion_before_a_deadline_rejects_its_result() {
    let runtime = runtime();
    let (block, release) = Block::new();
    runtime.block_on(async {
        let harness =
            Harness::with_phase_timeout(Duration::from_secs(2), Duration::from_millis(30));
        harness.probe.release();
        *harness.probe.block_context.lock().unwrap() = Some(block);
        let service = harness.endpoint.__managed_service().unwrap();
        let response = invoke(&harness.endpoint, &service, "{ ping }");
        until(|| harness.probe.entered.load(Ordering::SeqCst) == 1).await;
        assert_eq!(finite(response.await).status, 504);
        assert!(harness.probe.completions.lock().unwrap().is_empty());
        assert!(!harness.endpoint.managed_control().requests_idle());
        drop(release);
        until(|| harness.endpoint.managed_control().requests_idle()).await;
        assert_eq!(
            *harness.probe.completions.lock().unwrap(),
            [ResponseCompletionOutcome::TimedOut]
        );
        assert_eq!(harness.probe.completion_drops.load(Ordering::SeqCst), 1);
        assert_eq!(harness.probe.resolved.load(Ordering::SeqCst), 0);
        assert_eq!(harness.probe.hooks.load(Ordering::SeqCst), 0);
        assert_eq!(harness.endpoint.shutdown_outcome(), None);
        drop(service);
        joined(&harness.endpoint).await;
    });
}

#[test]
fn compatibility_finite_handler_drops_prepared_completion_uncalled_on_cancel() {
    let runtime = runtime();
    runtime.block_on(async {
        let harness = Harness::new(Duration::from_secs(2));
        harness.probe.release();
        harness
            .probe
            .pending_execution
            .store(true, Ordering::SeqCst);
        let metadata = chigiri_http::ApplicationMetadata::default();
        let headers = [chigiri_http::RequestHeader::new(
            "content-type",
            b"application/json",
        )];
        let request = chigiri_http::TransportRequest::new(
            "POST", "/graphql", None, &headers, None, &metadata,
        );
        let mut execution = harness.endpoint.handle(
            request,
            chigiri_http::BufferedBody::new(br#"{"query":"{ ping }"}"#.to_vec()),
            super::support::deadlines(),
        );
        std::future::poll_fn(|cx| {
            assert!(execution.as_mut().poll(cx).is_pending());
            if harness.probe.executing.load(Ordering::SeqCst) == 1 {
                std::task::Poll::Ready(())
            } else {
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            }
        })
        .await;
        drop(execution);
        assert!(harness.probe.completions.lock().unwrap().is_empty());
        assert_eq!(harness.probe.completion_drops.load(Ordering::SeqCst), 1);
        assert_eq!(harness.probe.resolver_drops.load(Ordering::SeqCst), 1);
        assert_eq!(harness.endpoint.shutdown_outcome(), None);
    });
}

#[test]
fn blocking_completion_stays_with_the_original_http_task_after_shutdown_expiry() {
    let runtime = runtime();
    let (block, release) = Block::new();
    runtime.block_on(async {
        let harness = Harness::new(Duration::from_secs(2));
        harness.probe.release();
        *harness.probe.block_completion.lock().unwrap() = Some(block);
        let service = harness.endpoint.__managed_service().unwrap();
        let response = invoke(&harness.endpoint, &service, "mutation { ping }");
        until(|| harness.probe.completions.lock().unwrap().len() == 1).await;
        harness
            .endpoint
            .shutdown_on(
                async {},
                ShutdownOptions::graceful_for(Duration::from_millis(20)).unwrap(),
            )
            .await;
        assert_eq!(
            harness.endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::Expired)
        );
        joined(&harness.endpoint).await;
        assert!(!harness.endpoint.managed_control().requests_idle());
        assert_eq!(harness.probe.completion_drops.load(Ordering::SeqCst), 0);
        assert!(matches!(
            response.await,
            crate::managed::ManagedHttpResponse::Empty { status: 503 }
        ));
        drop(release);
        until(|| harness.endpoint.managed_control().requests_idle()).await;
        assert_eq!(
            *harness.probe.completions.lock().unwrap(),
            [ResponseCompletionOutcome::Encoded]
        );
        assert_eq!(harness.probe.completion_drops.load(Ordering::SeqCst), 1);
        assert_eq!(
            harness.endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::Expired)
        );
        let weak = Arc::downgrade(&harness.probe);
        drop(service);
        drop(harness);
        assert!(weak.upgrade().is_none());
    });
}

#[test]
fn prepared_completion_runs_once_after_execution_is_cancelled() {
    let runtime = runtime();
    runtime.block_on(async {
        for immediate in [false, true] {
            let harness = Harness::new(Duration::from_secs(2));
            harness.probe.release();
            harness
                .probe
                .pending_execution
                .store(true, Ordering::SeqCst);
            let service = harness.endpoint.__managed_service().unwrap();
            let response = invoke(&harness.endpoint, &service, "mutation { ping }");
            until(|| harness.probe.executing.load(Ordering::SeqCst) == 1).await;
            if immediate {
                harness
                    .endpoint
                    .shutdown_on(async {}, ShutdownOptions::immediate())
                    .await;
            }
            drop(response);
            until(|| harness.endpoint.managed_control().requests_idle()).await;
            assert_eq!(
                *harness.probe.completions.lock().unwrap(),
                [ResponseCompletionOutcome::Failed]
            );
            assert_eq!(harness.probe.completion_drops.load(Ordering::SeqCst), 1);
            assert!(
                harness
                    .probe
                    .completion_saw_disposed_resolver
                    .load(Ordering::SeqCst)
            );
            assert_eq!(harness.probe.hooks.load(Ordering::SeqCst), 0);
            assert!(harness.probe.errors.lock().unwrap().is_empty());
            assert_eq!(
                harness.endpoint.shutdown_outcome(),
                immediate.then_some(ShutdownOutcome::CancellationRequested)
            );
            drop(service);
            joined(&harness.endpoint).await;
        }
    });
}

#[test]
fn original_and_shorter_hook_timeouts_complete_prepared_requests_once() {
    let runtime = runtime();
    runtime.block_on(async {
        for hook in [false, true] {
            let harness = Harness::with_phase_timeout(
                if hook {
                    Duration::from_secs(2)
                } else {
                    Duration::from_millis(30)
                },
                if hook {
                    Duration::from_millis(30)
                } else {
                    Duration::from_secs(1)
                },
            );
            harness.probe.release();
            if hook {
                harness.probe.pending_hook.store(true, Ordering::SeqCst);
            } else {
                harness
                    .probe
                    .pending_execution
                    .store(true, Ordering::SeqCst);
            }
            let service = harness.endpoint.__managed_service().unwrap();
            let response = invoke(&harness.endpoint, &service, "{ ping }");
            until(|| harness.endpoint.managed_control().requests_idle()).await;
            assert_eq!(finite(response.await).status, 504);
            assert_eq!(
                *harness.probe.completions.lock().unwrap(),
                [ResponseCompletionOutcome::TimedOut]
            );
            assert_eq!(harness.probe.completion_drops.load(Ordering::SeqCst), 1);
            assert_eq!(harness.endpoint.shutdown_outcome(), None);
            assert!(harness.probe.errors.lock().unwrap().is_empty());
            drop(service);
            joined(&harness.endpoint).await;
        }
    });
}

#[test]
fn ordinary_hook_failure_remains_observed_masked_and_completes_failed_once() {
    let runtime = runtime();
    runtime.block_on(async {
        let harness = Harness::new(Duration::from_secs(2));
        harness.probe.release();
        *harness.probe.mode.lock().unwrap() = Mode::HookError;
        let service = harness.endpoint.__managed_service().unwrap();
        let parts = finite(invoke(&harness.endpoint, &service, "{ ping }").await);
        assert_eq!(parts.status, 500);
        assert!(
            !String::from_utf8(parts.body)
                .unwrap()
                .contains("private hook failure marker")
        );
        assert_eq!(harness.probe.errors.lock().unwrap().len(), 1);
        assert_eq!(
            harness.probe.errors.lock().unwrap()[0].1,
            "private hook failure marker"
        );
        assert_eq!(
            *harness.probe.completions.lock().unwrap(),
            [ResponseCompletionOutcome::Failed]
        );
        assert_eq!(harness.probe.completion_drops.load(Ordering::SeqCst), 1);
        assert_eq!(harness.endpoint.shutdown_outcome(), None);
        drop(service);
        joined(&harness.endpoint).await;
    });
}

#[test]
fn worker_callback_and_destructor_panics_are_observed_without_retry_or_raw_output() {
    let runtime = runtime();
    runtime.block_on(async {
        for (mode, observer_panics) in [
            (Mode::ResolverPanic, false),
            (Mode::ResolverPanic, true),
            (Mode::CompletionPanic, false),
            (Mode::CompletionDropPanic, false),
        ] {
            let harness = Harness::new(Duration::from_secs(2));
            harness.probe.release();
            *harness.probe.mode.lock().unwrap() = mode;
            harness
                .probe
                .observer_panics
                .store(observer_panics, Ordering::SeqCst);
            let service = harness.endpoint.__managed_service().unwrap();
            let response = invoke(&harness.endpoint, &service, "{ ping }").await;
            assert!(matches!(
                response,
                crate::managed::ManagedHttpResponse::Empty { status: 500 }
            ));
            joined(&harness.endpoint).await;
            until(|| harness.endpoint.managed_control().requests_idle()).await;
            assert_eq!(harness.probe.errors.lock().unwrap().len(), 1);
            assert_eq!(
                harness.probe.errors.lock().unwrap()[0].0,
                "managed-http-execution"
            );
            assert_eq!(harness.probe.completions.lock().unwrap().len(), 1);
            assert_eq!(harness.probe.completion_drops.load(Ordering::SeqCst), 1);
            assert_eq!(
                harness.endpoint.shutdown_outcome(),
                Some(ShutdownOutcome::InfrastructureFailure)
            );
            drop(service);
        }
    });
}
