//! Contain network-loop unwind and join owned infrastructure cancellation.

use std::future::{Future, poll_fn};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::task::Poll;

use tokio::task::JoinSet;

pub(super) async fn contain(future: impl Future<Output = ()>) -> bool {
    let mut future = Box::pin(future);
    poll_fn(
        |context| match catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(context))) {
            Ok(Poll::Ready(())) => Poll::Ready(true),
            Ok(Poll::Pending) => Poll::Pending,
            Err(payload) => {
                // Only a fixed diagnostic escapes; never stringify an internal panic.
                let _ = catch_unwind(AssertUnwindSafe(|| drop(payload)));
                Poll::Ready(false)
            }
        },
    )
    .await
}

pub(super) async fn join(connections: &mut JoinSet<()>) -> bool {
    connections.abort_all();
    let mut clean = true;
    while let Some(joined) = connections.join_next().await {
        if let Err(error) = joined {
            clean &= error.is_cancelled();
        }
    }
    clean
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::pin::Pin;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::task::Context;

    use super::*;

    struct ConnectionValue(Arc<AtomicUsize>);

    impl Drop for ConnectionValue {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl http_body::Body for ConnectionValue {
        type Data = axum::body::Bytes;
        type Error = Infallible;

        fn poll_frame(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
            Poll::Pending
        }
    }

    #[tokio::test]
    async fn cutoff_and_unwind_join_destruction_before_later_runtime_drain() {
        for unwind in [false, true] {
            let drops = Arc::new(AtomicUsize::new(0));
            // Retain an actual unpolled HTTP body in the owned task. This
            // fixture proves task/body destruction; common byte/physical
            // lease ordering has its separate bridge supervision fixtures.
            let body = axum::body::Body::new(ConnectionValue(drops.clone()));
            let (started, receive) = tokio::sync::oneshot::channel();
            let mut connections = JoinSet::new();
            connections.spawn(async move {
                let _body = body;
                started.send(()).unwrap();
                std::future::pending::<()>().await;
            });
            receive.await.unwrap();
            assert_eq!(drops.load(Ordering::SeqCst), 0);
            let normal = contain(async {
                assert!(!unwind, "simulated network-loop failure");
            })
            .await;
            assert_eq!(normal, !unwind);
            assert!(join(&mut connections).await);
            assert!(connections.is_empty());
            // This checkpoint models entry to the later VM/writer drain:
            // cancellation has completed, not merely been requested.
            assert_eq!(drops.load(Ordering::SeqCst), 1);
        }
    }
}
