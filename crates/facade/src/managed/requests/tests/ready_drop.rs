//! Failed ready context destruction cannot lose an armed finite completion.

use std::process::Command;
use std::sync::atomic::Ordering;
use std::time::Duration;

use chigiri_http::ResponseCompletionOutcome;

use super::blocked::Block;
use super::support::{Harness, Mode, invoke, runtime, until};
use crate::{ShutdownOptions, ShutdownOutcome};

const CHILD: &str = "CHIGIRI_FINITE_READY_CONTEXT_DROP_CHILD";
const TEST: &str = "managed::requests::tests::ready_drop::failed_ready_context_future_keeps_the_unique_completion_and_request_owner";

#[test]
fn failed_ready_context_future_keeps_the_unique_completion_and_request_owner() {
    let Ok(case) = std::env::var(CHILD) else {
        for role in ["query", "mutation"] {
            for stop_first in [false, true] {
                let output = Command::new(std::env::current_exe().unwrap())
                    .args([TEST, "--exact", "--nocapture", "--test-threads=1"])
                    .env(CHILD, format!("{role}:{stop_first}"))
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "owned finite {role}:{stop_first} child failed: {}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
        return;
    };
    let (role, stop_first) = case.split_once(':').unwrap();
    let (block, release) = Block::new();
    runtime().block_on(async {
        let harness = Harness::new(Duration::from_secs(2));
        harness.probe.release();
        *harness.probe.mode.lock().unwrap() = Mode::ContextReadyDropPanic;
        *harness.probe.block_context.lock().unwrap() = Some(block);
        let service = harness.endpoint.__managed_service().unwrap();
        let response = invoke(
            &harness.endpoint,
            &service,
            if role == "query" {
                "query { ping }"
            } else {
                "mutation { ping }"
            },
        );
        until(|| harness.probe.entered.load(Ordering::SeqCst) == 1).await;
        let hook = if stop_first == "true" {
            let mut hook = harness
                .endpoint
                .shutdown_on(async {}, ShutdownOptions::graceful());
            assert!(
                std::future::poll_fn(|cx| std::task::Poll::Ready(hook.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
            Some(hook)
        } else {
            None
        };
        drop(release);
        until(|| harness.probe.completion_drops.load(Ordering::SeqCst) == 1).await;
        hook.unwrap_or_else(|| {
            harness
                .endpoint
                .shutdown_on(async {}, ShutdownOptions::graceful())
        })
        .await;
        assert!(matches!(
            response.await,
            crate::managed::ManagedHttpResponse::Empty { status: 500 | 503 }
        ));
        until(crate::subscription::worker::process_poisoned_for_test).await;
        assert_eq!(
            *harness.probe.completions.lock().unwrap(),
            [ResponseCompletionOutcome::Failed]
        );
        assert_eq!(harness.probe.context_future_drops.load(Ordering::SeqCst), 1);
        assert_eq!(harness.probe.context_drops.load(Ordering::SeqCst), 1);
        assert_eq!(harness.probe.resolved.load(Ordering::SeqCst), 0);
        assert_eq!(harness.probe.hooks.load(Ordering::SeqCst), 0);
        assert_eq!(harness.probe.errors.lock().unwrap().len(), 1);
        assert_eq!(
            harness.endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::InfrastructureFailure)
        );
        assert!(
            !harness.endpoint.managed_control().requests_idle(),
            "actual poisoned native join retains the original request count"
        );
        assert!(service.request().is_err());
    });
}
