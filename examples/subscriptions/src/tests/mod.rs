//! Native adoption proof; only documented SDK application APIs are used.

mod gate;
mod wire;

use std::panic::{AssertUnwindSafe, resume_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chigiri::{BoxFuture, ShutdownOptions, ShutdownOutcome};
use futures_util::FutureExt;
use gate::{Factory, Gate, Observe};
use tokio::task::JoinHandle;
use wire::Http;

use crate::{ALLOWED_ORIGIN, builder, domain::State, router};

const MESSAGE: &str = "An owned source shares this immutable domain backing.";

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    NoHook,
    Default,
    Override,
    Immediate,
    Expired,
    InitialFailure,
    EventFailure,
}

#[test]
fn native_query_sse_admission_disconnect_and_shutdown() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        for mode in [
            Mode::NoHook,
            Mode::Default,
            Mode::Override,
            Mode::Immediate,
            Mode::Expired,
            Mode::InitialFailure,
            Mode::EventFailure,
        ] {
            exercise(mode).await;
        }
    });
}

async fn until(predicate: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !predicate() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the example must reach its observed lifecycle boundary");
}

async fn joined(task: JoinHandle<std::io::Result<()>>) {
    let mut task = task;
    match tokio::time::timeout(Duration::from_secs(2), &mut task).await {
        Ok(result) => result.unwrap().unwrap(),
        Err(_) => {
            task.abort();
            let _ = task.await;
            panic!("the owned example server did not join");
        }
    }
}

async fn exercise(mode: Mode) {
    let state = State::default();
    let gate = Arc::new(Gate::default());
    let graphql = if mode == Mode::NoHook {
        crate::endpoint(state.clone()).unwrap()
    } else {
        builder(state.clone())
            .context_factory(Factory {
                gate: Arc::clone(&gate),
                hold: mode != Mode::NoHook,
                fail_setup: mode == Mode::InitialFailure,
                fail_event: mode == Mode::EventFailure,
            })
            .inject(|components| {
                components
                    .observability()
                    .error_observer(Observe(Arc::clone(&gate)));
            })
            .build()
            .unwrap()
    };
    let app = router(graphql.clone()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (signal, notification) = tokio::sync::oneshot::channel();
    let mut signal = Some(signal);
    let observed = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&observed);
    let notification = async move {
        let _ = notification.await;
        seen.store(true, Ordering::SeqCst);
    };
    let hook: BoxFuture<'static, ()> = match mode {
        Mode::NoHook => Box::pin(notification),
        _ => graphql.shutdown_on(
            notification,
            match mode {
                Mode::Immediate => ShutdownOptions::immediate(),
                Mode::Expired => ShutdownOptions::graceful_for(Duration::from_millis(250)).unwrap(),
                Mode::Override => ShutdownOptions::graceful_for(Duration::from_secs(2)).unwrap(),
                _ => ShutdownOptions::graceful(),
            },
        ),
    };
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(hook)
            .await
    });
    let mut stream = None;
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        AssertUnwindSafe(async {
            let query = Http::open(
                address,
                "/graphql",
                Some("{ current { number message } }"),
                Some(ALLOWED_ORIGIN),
                false,
            )
            .await;
            assert_eq!(query.status, 200);
            let data: serde_json::Value = serde_json::from_slice(&query.body().await).unwrap();
            assert_eq!(data["data"]["current"]["number"], 0);
            assert_eq!(data["data"]["current"]["message"], MESSAGE);
            for (path, origin, status) in [
                ("/wrong", None, 404),
                ("/graphql", Some("https://untrusted.example"), 403),
            ] {
                let denied = Http::open(
                    address,
                    path,
                    Some("subscription { readings { number } }"),
                    origin,
                    true,
                )
                .await;
                assert_eq!(denied.status, status);
                drop(denied.body().await);
            }
            assert_eq!(state.source_counts(), (0, 0, 0));
            assert_eq!(gate.setups.load(Ordering::SeqCst), 0);
            stream = Some(
                Http::open(
                    address,
                    "/graphql",
                    Some("subscription { readings { number message } }"),
                    Some(ALLOWED_ORIGIN),
                    true,
                )
                .await,
            );
            if mode == Mode::InitialFailure {
                let failed = stream.take().unwrap();
                assert_eq!(failed.status, 500);
                assert!(!failed.head.contains("text/event-stream"));
                assert!(failed.body().await.is_empty());
                return;
            }
            assert_eq!(stream.as_ref().unwrap().status, 200);
            if mode == Mode::NoHook {
                until(|| state.source_counts().0 == 1).await;
            } else {
                until(|| gate.entered.load(Ordering::SeqCst) == 1).await;
            }
            if mode == Mode::EventFailure {
                gate.release();
            } else if mode != Mode::NoHook {
                signal.take().unwrap().send(()).unwrap();
                until(|| observed.load(Ordering::SeqCst)).await;
                if matches!(mode, Mode::Default | Mode::Override) {
                    assert_eq!(graphql.shutdown_outcome(), None);
                    assert!(!server.is_finished());
                    let health = Http::open(address, "/health", None, None, false).await;
                    assert_eq!(health.status, 200);
                    assert_eq!(health.body().await, b"healthy");
                    let denied = Http::open(
                        address,
                        "/graphql",
                        Some("{ current { number } }"),
                        None,
                        false,
                    )
                    .await;
                    assert_eq!(denied.status, 503);
                    assert!(denied.body().await.is_empty());
                    gate.release();
                }
            }
            if matches!(mode, Mode::NoHook | Mode::Default | Mode::Override) {
                let event = stream.as_mut().unwrap().reading().await;
                assert_eq!(event["data"]["readings"]["number"], 1);
                assert_eq!(event["data"]["readings"]["message"], MESSAGE);
                assert_eq!(
                    event.pointer("/extensions/chigiri/delivery/droppedTotal"),
                    Some(&serde_json::Value::Null)
                );
                if mode == Mode::NoHook {
                    drop(stream.take());
                    until(|| state.source_counts().1 == 1).await;
                    assert_eq!(graphql.shutdown_outcome(), None);
                    return;
                }
            }
            let end = stream.as_mut().unwrap().raw().await;
            let wire = String::from_utf8_lossy(&end);
            assert!(!wire.contains("private example"));
            if matches!(mode, Mode::Default | Mode::Override) {
                assert!(end.ends_with(b"0\r\n\r\n"), "{mode:?}");
            } else if mode == Mode::Immediate {
                // Requested cancellation is an expected terminal, rather than
                // an infrastructure failure. The still-gated event cannot emit.
                assert!(!wire.contains("data: "), "{mode:?}");
            } else {
                assert!(!end.ends_with(b"0\r\n\r\n"), "{mode:?}");
                assert!(!wire.contains("event: complete"));
            }
        })
        .catch_unwind(),
    )
    .await;
    gate.release();
    drop(stream.take());
    if let Some(signal) = signal.take() {
        let _ = signal.send(());
    }
    joined(server).await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(payload)) => resume_unwind(payload),
        Err(error) => panic!("the bounded example exchange did not complete: {error}"),
    }
    let failure = matches!(mode, Mode::InitialFailure | Mode::EventFailure);
    assert_eq!(
        gate.setups.load(Ordering::SeqCst),
        usize::from(mode != Mode::NoHook)
    );
    assert_eq!(gate.errors.load(Ordering::SeqCst), usize::from(failure));
    let expected = match mode {
        Mode::NoHook | Mode::Immediate => ShutdownOutcome::CancellationRequested,
        Mode::Expired => ShutdownOutcome::Expired,
        Mode::InitialFailure | Mode::EventFailure => ShutdownOutcome::InfrastructureFailure,
        _ => ShutdownOutcome::Drained,
    };
    until(|| graphql.shutdown_outcome().is_some()).await;
    assert_eq!(graphql.shutdown_outcome(), Some(expected), "{mode:?}");
    if mode == Mode::InitialFailure {
        assert_eq!(state.source_counts(), (0, 0, 0));
    } else {
        until(|| state.source_counts().1 == 1).await;
        assert_eq!(state.source_counts(), (1, 1, 0));
    }
}
