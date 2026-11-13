//! Public-only hooks exercise both native server shutdown boundaries.

use std::panic::{AssertUnwindSafe, resume_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chigiri::{ShutdownOptions, ShutdownOutcome};
use futures_util::FutureExt;
use tokio::task::JoinHandle;

use super::fixture::{Probe, endpoint};
use super::{client, hyper_server};

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Graceful,
    Expired,
    Failed,
    Immediate,
}

pub(crate) fn run() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        for hyper in [false, true] {
            for mode in [Mode::Graceful, Mode::Expired, Mode::Failed, Mode::Immediate] {
                exercise(mode, hyper).await;
            }
        }
    });
}

pub(super) async fn wait_until(predicate: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !predicate() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the native fixture must reach its observed lifecycle boundary");
}

pub(super) async fn join<T>(task: &mut Option<JoinHandle<T>>) -> Result<T, String> {
    let result = tokio::time::timeout(Duration::from_secs(2), task.as_mut().unwrap()).await;
    let task = task.take().unwrap();
    match result {
        Ok(result) => result.map_err(|error| error.to_string()),
        Err(_) => {
            task.abort();
            let _ = task.await;
            Err("owned native fixture task did not complete".to_owned())
        }
    }
}

async fn exercise(mode: Mode, hyper: bool) {
    let probe = Probe::new(mode == Mode::Failed);
    let graphql = endpoint(Arc::clone(&probe));
    let independent = Probe::new(false);
    independent.release();
    let other = endpoint(Arc::clone(&independent));
    let (app, native) = if hyper {
        (
            None,
            Some((
                chigiri_hyper::managed_service(graphql.clone()).unwrap(),
                chigiri_hyper::managed_service(other.clone()).unwrap(),
            )),
        )
    } else {
        (
            Some(
                axum::Router::new()
                    .route_service("/graphql", chigiri_axum::service(graphql.clone()).unwrap())
                    .route_service(
                        "/other-graphql",
                        chigiri_axum::service(other.clone()).unwrap(),
                    )
                    .route("/health", axum::routing::get(|| async { "healthy" })),
            ),
            None,
        )
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (signal, notification) = tokio::sync::oneshot::channel();
    let observed = Arc::new(AtomicBool::new(false));
    let signal_observed = Arc::clone(&observed);
    let options = match mode {
        Mode::Immediate => ShutdownOptions::immediate(),
        Mode::Expired => ShutdownOptions::graceful_for(Duration::from_millis(500)).unwrap(),
        _ => ShutdownOptions::graceful(),
    };
    let hook = graphql.shutdown_on(
        async move {
            let _ = notification.await;
            signal_observed.store(true, Ordering::SeqCst);
        },
        options,
    );
    let mut server = Some(tokio::spawn(async move {
        if let Some((graphql, other)) = native {
            hyper_server::serve(listener, graphql, other, hook).await
        } else {
            axum::serve(listener, app.unwrap())
                .with_graceful_shutdown(hook)
                .await
        }
    }));
    let mut execution = Some(tokio::spawn(client::request(
        address,
        "/graphql",
        Some("query { greeting }"),
    )));
    let result = AssertUnwindSafe(async {
        wait_until(|| probe.calls.load(Ordering::SeqCst) == 1).await;
        signal.send(()).unwrap();
        wait_until(|| observed.load(Ordering::SeqCst)).await;
        if mode != Mode::Immediate {
            assert!(!server.as_ref().unwrap().is_finished());
            assert_eq!(graphql.shutdown_outcome(), None);
            let health = client::request(address, "/health", None).await.unwrap();
            assert_eq!(health.status, 200);
            assert_eq!(health.body, b"healthy");
            let rejected = client::request(address, "/graphql", Some("query { greeting }"))
                .await
                .unwrap();
            assert_eq!(rejected.status, 503);
            assert!(rejected.body.is_empty());
            let separate = client::request(address, "/other-graphql", Some("query { greeting }"))
                .await
                .unwrap();
            assert_eq!(separate.status, 200);
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&separate.body).unwrap(),
                serde_json::json!({"data": {"greeting": "ready"}})
            );
            assert_eq!(other.shutdown_outcome(), None);
            assert!(!server.as_ref().unwrap().is_finished());
        }
        if matches!(mode, Mode::Graceful | Mode::Failed) {
            probe.release();
        }
        let response = join(&mut execution).await.unwrap().unwrap();
        let expected = match mode {
            Mode::Graceful => ShutdownOutcome::Drained,
            Mode::Expired => ShutdownOutcome::Expired,
            Mode::Failed => ShutdownOutcome::InfrastructureFailure,
            Mode::Immediate => ShutdownOutcome::CancellationRequested,
        };
        if mode == Mode::Graceful {
            assert_eq!(response.status, 200);
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&response.body).unwrap(),
                serde_json::json!({"data": {"greeting": "ready"}})
            );
        } else {
            assert_eq!(
                response.status,
                if mode == Mode::Failed { 500 } else { 503 }
            );
            assert!(response.body.is_empty());
        }
        join(&mut server).await.unwrap().unwrap();
        wait_until(|| probe.dropped.load(Ordering::SeqCst) == 1).await;
        assert_eq!(graphql.shutdown_outcome(), Some(expected));
        assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            probe.errors.load(Ordering::SeqCst),
            usize::from(mode == Mode::Failed)
        );
        assert_eq!(probe.sources.load(Ordering::SeqCst), 0);
    })
    .catch_unwind()
    .await;
    probe.release();
    let execution_result = if execution.is_some() {
        Some(join(&mut execution).await)
    } else {
        None
    };
    let server_result = if server.is_some() {
        Some(join(&mut server).await)
    } else {
        None
    };
    if let Err(payload) = result {
        resume_unwind(payload);
    }
    assert!(execution_result.is_none() && server_result.is_none());
}
