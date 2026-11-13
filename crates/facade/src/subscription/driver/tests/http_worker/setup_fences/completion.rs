//! A cancellation callback stays in the original charged concrete task.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chigiri_http::{
    BoxFuture, ContextFactory, PrepareError, PreparedContext, RequestContext, ResponseCompletion,
    ResponseCompletionOutcome, ResponseParts,
};
use chigiri_runtime::LifecyclePhase;

use super::super::super::harness::Factory;
use super::super::super::state::{AppContext, HookState, SourceStep, State, count};
use super::super::support::{VALID, profiles, run};
use super::stop;
use super::support::{Fixture, reached};
use crate::ShutdownOutcome;
use crate::transport::streaming::PreparedHttpSubscription;

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

struct Callback {
    entered: AtomicUsize,
    release: Mutex<Receiver<()>>,
    panic: bool,
}

struct WrappedFactory {
    inner: Factory,
    callback: Arc<Callback>,
}
impl ContextFactory<AppContext, HookState> for WrappedFactory {
    fn create<'a>(
        &'a self,
        request: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<AppContext, HookState>, PrepareError>> {
        Box::pin(async move {
            let prepared = self.inner.create(request).await?;
            let (context, state, completion) = prepared.__into_parts();
            Ok(
                PreparedContext::new(context, state).with_response_completion(WrappedCompletion {
                    inner: completion.unwrap(),
                    callback: self.callback.clone(),
                }),
            )
        })
    }
}

struct WrappedCompletion {
    inner: Arc<dyn ResponseCompletion>,
    callback: Arc<Callback>,
}
impl ResponseCompletion for WrappedCompletion {
    fn complete(
        &self,
        response: ResponseParts,
        outcome: ResponseCompletionOutcome,
    ) -> ResponseParts {
        let response = self.inner.complete(response, outcome);
        assert_eq!(self.callback.entered.fetch_add(1, Ordering::SeqCst), 0);
        if self.callback.panic {
            panic!("private cancellation completion panic");
        }
        self.callback
            .release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(2))
            .expect("owned callback release");
        response
    }
}

fn install(fixture: &mut Fixture, probe: &Arc<State>, panic: bool) -> (Arc<Callback>, Release) {
    let (release, released) = sync_channel(1);
    let callback = Arc::new(Callback {
        entered: AtomicUsize::new(0),
        release: Mutex::new(released),
        panic,
    });
    fixture.replace_factory(WrappedFactory {
        inner: Factory(probe.clone()),
        callback: callback.clone(),
    });
    (callback, Release(Some(release)))
}

#[test]
fn cancellation_completion_panic_is_one_private_failure_not_a_successful_cancel() {
    for profile in profiles() {
        let probe = State::new(vec![SourceStep::Pending]);
        let mut fixture = Fixture::new(probe.clone(), Some(LifecyclePhase::Execution));
        let (callback, _release) = install(&mut fixture, &probe, true);
        let mut preparation = Box::pin(fixture.preparation(profile, VALID));
        fixture.reach(preparation.as_mut(), || reached(&fixture.boundary));
        stop(&fixture, 1);
        fixture.release();
        fixture.finished();
        let PreparedHttpSubscription::Response(response) = run(&fixture.runtime, preparation)
        else {
            panic!("masked callback failure");
        };
        assert_eq!(response.status(), 500);
        assert!(!String::from_utf8_lossy(response.as_ref()).contains("private cancellation"));
        assert_eq!(count(&callback.entered), 1);
        assert_eq!(count(&probe.counts.completion_calls), 1);
        assert_eq!(count(&probe.counts.completion_drops), 1);
        assert_eq!(probe.completion_drop_counts.lock().unwrap()[0][0], 1);
        assert_eq!(count(&fixture.boundary.errors), 1);
        assert_eq!(fixture.cancelled_requests(), 0);
        drop(response);
        fixture.finish();
    }
}

#[test]
fn blocked_cancellation_completion_keeps_original_permit_and_bytes_through_expiry_and_abandonment()
{
    for profile in profiles() {
        let probe = State::new(vec![SourceStep::Pending]);
        let mut fixture = Fixture::new(probe.clone(), Some(LifecyclePhase::Execution));
        let (callback, mut release) = install(&mut fixture, &probe, false);
        let mut preparation = Box::pin(fixture.preparation(profile, VALID));
        fixture.reach(preparation.as_mut(), || reached(&fixture.boundary));
        stop(&fixture, 1);
        fixture.release();
        fixture.runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(1), async {
                while count(&callback.entered) == 0 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("original task must enter cancellation callback");
        });
        assert!(!fixture.task_finished());
        let endpoint = fixture.endpoint.clone();
        let delivery = endpoint.subscriptions.resources.delivery();
        let bytes = delivery.byte_owner();
        assert_eq!(delivery.snapshot().endpoint_active_subscriptions, 1);
        assert!(bytes.used() > 0);
        fixture
            .control
            .expire(tokio::time::Instant::now() + Duration::from_secs(31));
        assert_eq!(fixture.control.outcome(), Some(ShutdownOutcome::Expired));
        drop(preparation);
        assert_eq!(delivery.snapshot().endpoint_active_subscriptions, 1);
        assert!(bytes.used() > 0);
        assert_eq!(count(&probe.counts.source_drops), 1);
        assert_eq!(count(&probe.counts.completion_calls), 1);
        release.send();
        fixture.finished();
        assert_eq!(fixture.control.outcome(), Some(ShutdownOutcome::Expired));
        assert_eq!(count(&probe.counts.completion_drops), 1);
        fixture.finish();
        assert_eq!(bytes.used(), 0);
        assert_eq!(delivery.snapshot().endpoint_active_subscriptions, 0);
    }
}
