//! A shutdown accepted between worker polling and emission must make progress.

use super::*;
use std::sync::mpsc::{SyncSender, sync_channel};

struct Release(Option<SyncSender<()>>);
impl Release {
    fn send(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        self.send();
    }
}

#[test]
fn source_cancelled_after_poll_start_cannot_spin_the_graceful_emission_fence() {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new(vec![SourceStep::Pending]);
        let endpoint = session_endpoint(probe.clone(), SubscriptionLimits::default());
        run(&runtime, async {
            let service = endpoint.__managed_service().unwrap();
            let mut body = body(&endpoint, &service, profile).await;
            drop(next(&mut body).await.unwrap());
            until(&probe.counts.source_polls, 1).await;
            let (entered, enter) = sync_channel(1);
            let (released, receive_release) = sync_channel(1);
            let mut release = Release(Some(released));
            body.before_worker_for_test(Box::new(move || {
                entered.send(()).unwrap();
                // The source cleanup deadline starts after this hook. Release also
                // unblocks this wait if the owning fixture's watchdog unwinds.
                receive_release
                    .recv()
                    .expect("owned worker barrier release");
            }));
            let control = endpoint.managed_control();
            control.notify();
            enter
                .recv_timeout(Duration::from_secs(2))
                .expect("SDK must reach the out-of-lock worker barrier");
            let mut shutdown = endpoint.shutdown_on(async {}, ShutdownOptions::graceful());
            assert!(
                poll_fn(|cx| std::task::Poll::Ready(shutdown.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
            until(&probe.counts.source_drops, 1).await;
            release.send();
            let result = tokio::time::timeout(Duration::from_secs(1), shutdown).await;
            if result.is_err() {
                control.stop();
            }
            assert!(
                result.is_ok(),
                "accepted source cancellation must leave a progressing graceful owner"
            );
            assert_eq!(endpoint.shutdown_outcome(), Some(ShutdownOutcome::Drained));
            assert!(next(&mut body).await.is_none());
            assert_eq!(count(&probe.counts.source_drops), 1);
            assert_eq!(count(&probe.counts.factory_calls), 1);
            assert_eq!(count(&probe.counts.hook_calls), 1);
            assert_eq!(count(&probe.counts.completion_calls), 1);
        });
    }
}
