//! Finite HTTP ownership is independent of native response polling.

mod application;
mod blocked;
mod completion;
mod ingress;
mod phases;
mod poison;
mod ready_drop;
mod support;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::{ShutdownOptions, ShutdownOutcome};
use support::{Harness, invoke, runtime, until};

#[test]
fn graceful_query_and_mutation_finish_without_polling_the_native_waiter() {
    let runtime = runtime();
    runtime.block_on(async {
        for document in ["query { ping }", "mutation { ping }"] {
            let harness = Harness::new(Duration::from_secs(2));
            let service = harness.endpoint.__managed_service().unwrap();
            let response = invoke(&harness.endpoint, &service, document);
            until(|| harness.probe.entered.load(Ordering::SeqCst) == 1).await;
            let mut shutdown = harness.endpoint.shutdown_on(
                async {},
                ShutdownOptions::graceful_for(Duration::from_secs(1)).unwrap(),
            );
            assert!(
                std::future::poll_fn(|cx| {
                    std::task::Poll::Ready(shutdown.as_mut().poll(cx).is_pending())
                })
                .await
            );
            assert!(service.request().is_err());
            assert!(!harness.endpoint.managed_control().requests_idle());
            harness.probe.release();
            tokio::time::timeout(Duration::from_secs(2), shutdown)
                .await
                .unwrap();
            assert_eq!(
                harness.endpoint.shutdown_outcome(),
                Some(ShutdownOutcome::Drained)
            );
            assert!(harness.endpoint.managed_control().requests_idle());
            assert_eq!(harness.probe.resolved.load(Ordering::SeqCst), 1);
            assert_eq!(harness.probe.hooks.load(Ordering::SeqCst), 1);
            assert_eq!(
                *harness.probe.completions.lock().unwrap(),
                [chigiri_http::ResponseCompletionOutcome::Encoded]
            );
            let parts = support::finite(response.await);
            assert_eq!(parts.status, 200);
            assert_eq!(
                String::from_utf8(parts.body).unwrap(),
                "{\"data\":{\"ping\":\"pong\"}}"
            );
            drop(service);
            support::joined(&harness.endpoint).await;
        }
    });
}

#[test]
fn dropping_an_unpolled_waiter_cancels_owned_http_without_stopping_the_endpoint() {
    let runtime = runtime();
    runtime.block_on(async {
        let harness = Harness::new(Duration::from_secs(2));
        let service = harness.endpoint.__managed_service().unwrap();
        let response = invoke(&harness.endpoint, &service, "{ ping }");
        until(|| harness.probe.entered.load(Ordering::SeqCst) == 1).await;
        drop(response);
        until(|| harness.endpoint.managed_control().requests_idle()).await;
        assert_eq!(harness.endpoint.shutdown_outcome(), None);
        assert_eq!(harness.probe.resolved.load(Ordering::SeqCst), 0);
        assert_eq!(harness.probe.context_drops.load(Ordering::SeqCst), 1);
        assert!(harness.probe.completions.lock().unwrap().is_empty());
        drop(service.request().unwrap());
        drop(service);
        support::joined(&harness.endpoint).await;
    });
}

#[test]
fn overall_timeout_runs_without_a_native_poll_and_does_not_cancel_another_endpoint() {
    let runtime = runtime();
    runtime.block_on(async {
        let harness = Harness::new(Duration::from_millis(30));
        let service = harness.endpoint.__managed_service().unwrap();
        let other = Harness::new(Duration::from_secs(2));
        let independent = other.endpoint.__managed_service().unwrap();
        let response = invoke(&harness.endpoint, &service, "{ ping }");
        until(|| harness.probe.entered.load(Ordering::SeqCst) == 1).await;
        until(|| harness.endpoint.managed_control().requests_idle()).await;
        assert_eq!(harness.probe.context_drops.load(Ordering::SeqCst), 1);
        let parts = support::finite(response.await);
        assert_eq!(parts.status, 504);
        assert!(
            String::from_utf8(parts.body)
                .unwrap()
                .contains("REQUEST_TIMEOUT")
        );
        assert_eq!(harness.endpoint.shutdown_outcome(), None);
        assert_eq!(other.endpoint.shutdown_outcome(), None);
        drop(independent.request().unwrap());
        drop(service);
        drop(independent);
        support::joined(&harness.endpoint).await;
        support::joined(&other.endpoint).await;
    });
}

#[test]
fn an_unpolled_response_does_not_keep_the_last_service_owner_alive() {
    let runtime = runtime();
    runtime.block_on(async {
        let harness = Harness::new(Duration::from_secs(2));
        let service = harness.endpoint.__managed_service().unwrap();
        let response = invoke(&harness.endpoint, &service, "{ ping }");
        until(|| harness.probe.entered.load(Ordering::SeqCst) == 1).await;
        drop(service);
        assert_eq!(
            harness.endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::CancellationRequested)
        );
        assert!(matches!(
            response.await,
            crate::managed::ManagedHttpResponse::Empty { status: 503 }
        ));
        support::joined(&harness.endpoint).await;
        assert!(harness.endpoint.managed_control().requests_idle());
        let weak = Arc::downgrade(&harness.probe);
        drop(harness);
        assert!(
            weak.upgrade().is_none(),
            "retired HTTP work must not retain the endpoint graph"
        );
    });
}
