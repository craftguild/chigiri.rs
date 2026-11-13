//! Public bridge failures retain native status, framing and private observation.

mod server;
mod wire;

use std::panic::{AssertUnwindSafe, resume_unwind};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use chigiri::{ShutdownOptions, ShutdownOutcome};
use futures_util::FutureExt;

use super::cases::{join, wait_until};
use super::fixture::{Probe, endpoint};

pub(crate) fn run() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        for hyper in [false, true] {
            for multipart in [false, true] {
                for after_head in [false, true] {
                    exercise(hyper, multipart, after_head).await;
                }
            }
        }
    });
}

async fn exercise(hyper: bool, multipart: bool, after_head: bool) {
    let probe = Probe::subscription_failure(after_head);
    let graphql = endpoint(Arc::clone(&probe));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (signal, notification) = tokio::sync::oneshot::channel();
    let hook = graphql.shutdown_on(
        async move {
            let _ = notification.await;
        },
        ShutdownOptions::graceful(),
    );
    let mut native = Some(server::start(hyper, graphql.clone(), listener, hook));
    let checked = AssertUnwindSafe(wire::exchange(address, multipart, after_head, &probe))
        .catch_unwind()
        .await;
    // Always release and join the owned native task before propagating a failed
    // assertion; no successful-path-only listener or connection cleanup.
    probe.release();
    let _ = signal.send(());
    let joined = join(&mut native).await;
    let bytes = match checked {
        Ok(bytes) => bytes,
        Err(payload) => resume_unwind(payload),
    };
    let result = joined.unwrap();
    if after_head {
        let error = result.expect_err("a failed committed body must fail its native connection");
        assert!(
            error.is_user(),
            "the native error must originate in the body"
        );
        wire::assert_abnormal(&bytes, multipart);
    } else {
        result.expect("the masked initial 500 is a complete finite HTTP response");
        wire::assert_finite(&bytes);
    }
    wait_until(|| probe.errors.load(Ordering::SeqCst) == 1).await;
    assert_eq!(probe.sources.load(Ordering::SeqCst), 1);
    assert_eq!(probe.calls.load(Ordering::SeqCst), usize::from(after_head));
    assert_eq!(
        probe.dropped.load(Ordering::SeqCst),
        usize::from(after_head)
    );
    assert_eq!(
        graphql.shutdown_outcome(),
        Some(ShutdownOutcome::InfrastructureFailure)
    );
    assert_eq!(probe.errors.load(Ordering::SeqCst), 1);
}
