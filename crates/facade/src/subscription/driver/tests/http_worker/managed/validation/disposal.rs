//! Last application-owner destruction must stay in an original SDK task.

mod poison;

use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

use chigiri_http::{ContextFactory, PrepareError, PreparedContext, RequestContext};

use super::super::super::super::state::HookState;
use super::*;

const VALIDATION: &[u8] = br#"{"query":"subscription Named($value: Probe!) { events(limit: 2, probe: $value) { value } }","operationName":"Named","variables":{"value":"worker-value"}}"#;

struct DropState {
    entered: AtomicUsize,
    in_task: AtomicBool,
    release: Mutex<Receiver<()>>,
    poison: bool,
}

struct DropRelease(Option<SyncSender<()>>);
impl DropRelease {
    fn send(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}
impl Drop for DropRelease {
    fn drop(&mut self) {
        self.send();
    }
}

struct LastFactory(Factory, Arc<DropState>);
impl ContextFactory<AppContext, HookState> for LastFactory {
    fn create<'a>(
        &'a self,
        request: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<AppContext, HookState>, PrepareError>> {
        self.0.create(request)
    }
}
impl Drop for LastFactory {
    fn drop(&mut self) {
        self.1
            .in_task
            .store(tokio::task::try_id().is_some(), Ordering::SeqCst);
        self.1.entered.fetch_add(1, Ordering::SeqCst);
        self.1
            .release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(2))
            .expect("release the application destructor");
        if self.1.poison {
            panic!("private validation graph destruction failure");
        }
    }
}

#[test]
fn abandoned_validation_drops_the_last_factory_inside_a_charged_sdk_task() {
    for profile in profiles() {
        exercise(profile, false);
    }
}

fn exercise(profile: SubscriptionTransport, poison: bool) {
    let runtime = runtime();
    let probe = State::new(Vec::new());
    let scalar_release = Release(probe.block_scalar());
    let (release, released) = sync_channel(1);
    let mut drop_release = DropRelease(Some(release));
    let drops = Arc::new(DropState {
        entered: AtomicUsize::new(0),
        in_task: AtomicBool::new(false),
        release: Mutex::new(released),
        poison,
    });
    let errors = Arc::new(AtomicUsize::new(0));
    let schema = ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(probe.clone()))
        .build()
        .unwrap();
    let endpoint = Endpoint::builder(schema)
        .context_factory(LastFactory(Factory(probe.clone()), drops.clone()))
        .response_hook(Hook(probe.clone()))
        .inject(|components| {
            components
                .observability()
                .error_observer(Errors(errors.clone()));
        })
        .build()
        .unwrap();
    let physical = endpoint.subscription_pending_probe_for_test();
    let control = endpoint.managed_control();
    run(&runtime, async {
        let service = endpoint.__managed_service().unwrap();
        let response = invoke(&endpoint, &service, profile, VALIDATION);
        until(&probe.counts.scalar_calls, 1).await;
        let mut shutdown = endpoint.shutdown_on(
            async {},
            ShutdownOptions::graceful_for(Duration::from_millis(20)).unwrap(),
        );
        assert!(
            poll_fn(|cx| std::task::Poll::Ready(shutdown.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        drop(response);
        drop(endpoint);
        tokio::time::timeout(Duration::from_secs(2), async {
            while !control.requests_idle() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the outside SDK HTTP task must join first");
        scalar_release.0.release();
        until(&drops.entered, 1).await;
        assert!(
            drops.in_task.load(Ordering::SeqCst),
            "application destruction cannot run on process recovery"
        );
        shutdown.await;
        assert_eq!(control.outcome(), Some(ShutdownOutcome::Expired));
        let (pending, bytes) = physical();
        assert_eq!(
            pending, 1,
            "blocked destruction retains the original pending permit"
        );
        assert!(
            bytes > 0,
            "blocked destruction retains the original byte guards"
        );
        drop_release.send();
        if poison {
            tokio::time::timeout(Duration::from_secs(2), async {
                while !crate::subscription::worker::process_poisoned_for_test() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "native JoinReady must preserve the poisoned record; error={error}, {}",
                    crate::subscription::worker::process_diagnostic_for_test()
                )
            });
            let (pending, bytes) = physical();
            assert_eq!(
                pending, 1,
                "failed disposal cannot release its original permit"
            );
            assert!(bytes > 0, "failed disposal retains original byte guards");
            assert_eq!(count(&errors), 1);
        } else {
            tokio::time::timeout(Duration::from_secs(2), async {
                while physical() != (0, 0) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("original physical owners release only after actual join");
            assert_eq!(count(&errors), 0);
        }
        assert_eq!(control.outcome(), Some(ShutdownOutcome::Expired));
        assert_eq!(count(&probe.counts.factory_calls), 0);
        assert_eq!(count(&probe.counts.setup_calls), 0);
        assert_eq!(count(&probe.counts.hook_calls), 0);
        assert_eq!(count(&probe.counts.completion_calls), 0);
        assert_eq!(count(&drops.entered), 1);
        drop(service);
    });
}
