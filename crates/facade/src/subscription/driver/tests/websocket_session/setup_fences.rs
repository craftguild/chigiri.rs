//! Pending workers must observe stop before their next application phase.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::task::{Context, Waker};
use std::time::Duration;

use chigiri_runtime::{
    ErrorEvent, ErrorObserver, LifecycleEvent, LifecycleObserver, LifecyclePhase, LifecycleResult,
};

use super::super::harness::observed_session_endpoint;
use super::super::state::{EventMode, FactoryMode, HookMode, SetupMode, SourceStep, State, count};
use super::support::{Fixture, SUBSCRIPTION, messages_for};
use crate::ShutdownOptions;
use crate::managed::Control;

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

struct Boundary {
    phase: LifecyclePhase,
    reached: AtomicUsize,
    errors: AtomicUsize,
    release: Mutex<Receiver<()>>,
    seen: Mutex<Vec<(LifecyclePhase, LifecycleResult)>>,
}

#[derive(Clone)]
struct Observe(Arc<Boundary>);

impl LifecycleObserver for Observe {
    fn observe(&self, event: LifecycleEvent<'_>) {
        self.0
            .seen
            .lock()
            .unwrap()
            .push((event.phase(), event.result()));
        if event.phase() == self.0.phase && event.result() == LifecycleResult::Succeeded {
            assert_eq!(self.0.reached.fetch_add(1, Ordering::SeqCst), 0);
            self.0
                .release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(2))
                .expect("owned phase barrier must be released");
        }
    }
}
impl ErrorObserver for Observe {
    fn observe_error(&self, _: ErrorEvent<'_>) {
        self.0.errors.fetch_add(1, Ordering::SeqCst);
    }
}

fn reach(fixture: &mut Fixture, boundary: &Boundary) {
    let driver = fixture.driver.as_mut().unwrap();
    fixture.runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(1), async {
            while count(&boundary.reached) == 0 {
                assert!(
                    Pin::new(&mut *driver)
                        .poll(&mut Context::from_waker(Waker::noop()))
                        .is_pending()
                );
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "worker barrier {:?}: seen={:?}, sent={:?}",
                boundary.phase,
                boundary.seen.lock().unwrap(),
                fixture.socket.messages()
            )
        });
    });
}

fn finished_without_connection_poll(fixture: &Fixture) {
    let driver = fixture.driver.as_ref().unwrap();
    fixture.runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(1), async {
            while !driver.setup_finished_for_test("pending") {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("task-side cancellation must complete independently");
    });
}

#[test]
fn accepted_stop_blocks_next_validation_or_source_phase_without_connection_polling() {
    for phase in [
        LifecyclePhase::OperationSourceResolution,
        LifecyclePhase::DecodedRequestHook,
        LifecyclePhase::ContextConstruction,
    ] {
        for options in [ShutdownOptions::immediate(), ShutdownOptions::graceful()] {
            let probe = State::new(vec![SourceStep::Pending]);
            let (release, released) = sync_channel(1);
            let mut release = Release(Some(release));
            let boundary = Arc::new(Boundary {
                phase,
                reached: AtomicUsize::new(0),
                errors: AtomicUsize::new(0),
                release: Mutex::new(released),
                seen: Mutex::new(Vec::new()),
            });
            let endpoint = observed_session_endpoint(
                probe.clone(),
                Observe(boundary.clone()),
                Observe(boundary.clone()),
            );
            let mut fixture = Fixture::with_endpoint(endpoint);
            let control = Control::new();
            control.activation_started().unwrap();
            fixture
                .driver
                .as_ref()
                .unwrap()
                .bind_managed_for_test(&control);
            if phase == LifecyclePhase::ContextConstruction {
                fixture.socket.subscribe("pending", SUBSCRIPTION);
            } else {
                // Scalar coercion is application work, even before context.
                fixture.socket.text(r#"{"id":"pending","type":"subscribe","payload":{"query":"query Named($value: Probe!) { echo(value: $value) }","operationName":"Named","variables":{"value":"worker-value"}}}"#);
            }
            reach(&mut fixture, &boundary);
            control.begin(options, tokio::time::Instant::now());
            release.send();
            // Do not poll the connection: the concrete task itself must stop
            // before coercion/context/source without outside cancellation.
            finished_without_connection_poll(&fixture);
            assert_eq!(count(&probe.counts.setup_calls), 0);
            assert_eq!(count(&probe.counts.source_polls), 0);
            assert_eq!(count(&probe.counts.scalar_calls), 0);
            let contexts = usize::from(phase == LifecyclePhase::ContextConstruction);
            assert_eq!(count(&probe.counts.factory_calls), contexts);
            assert_eq!(count(&probe.counts.setup_context_drops), contexts);
            assert_eq!(count(&probe.counts.completion_drops), contexts);
            assert_eq!(count(&probe.counts.completion_calls), 0);
            fixture.until_closed();
            assert!(messages_for(&fixture.socket, "pending").is_empty());
            assert_eq!(count(&boundary.errors), 0);
            fixture.finish();
        }
    }
}

#[test]
fn pending_context_and_source_setup_cancel_in_the_original_task_without_connection_polling() {
    for source_setup in [false, true] {
        for options in [ShutdownOptions::immediate(), ShutdownOptions::graceful()] {
            let probe = State::modes(
                Vec::new(),
                if source_setup {
                    SetupMode::Pending
                } else {
                    SetupMode::Ready
                },
                EventMode::Object,
                if source_setup {
                    FactoryMode::Ready
                } else {
                    FactoryMode::Pending
                },
                FactoryMode::Ready,
                HookMode::Ready,
            );
            let mut fixture = Fixture::new(probe.clone());
            let control = Control::new();
            control.activation_started().unwrap();
            fixture
                .driver
                .as_ref()
                .unwrap()
                .bind_managed_for_test(&control);
            fixture.socket.subscribe("pending", SUBSCRIPTION);
            let counter = if source_setup {
                &probe.counts.setup_calls
            } else {
                &probe.counts.factory_calls
            };
            let driver = fixture.driver.as_mut().unwrap();
            fixture.runtime.block_on(async {
                tokio::time::timeout(Duration::from_secs(1), async {
                    while count(counter) != 1 {
                        assert!(
                            Pin::new(&mut *driver)
                                .poll(&mut Context::from_waker(Waker::noop()))
                                .is_pending()
                        );
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("fixture must enter pending application setup");
            });
            control.begin(options, tokio::time::Instant::now());
            finished_without_connection_poll(&fixture);
            if source_setup {
                assert_eq!(count(&probe.counts.setup_future_drops), 1);
                assert_eq!(count(&probe.counts.setup_context_drops), 1);
                assert_eq!(count(&probe.counts.completion_drops), 1);
            } else {
                assert_eq!(count(&probe.counts.factory_future_drops), 1);
                assert_eq!(count(&probe.counts.setup_calls), 0);
            }
            assert_eq!(count(&probe.counts.source_polls), 0);
            assert_eq!(count(&probe.counts.completion_calls), 0);
            fixture.until_closed();
            assert!(messages_for(&fixture.socket, "pending").is_empty());
            fixture.finish();
        }
    }
}
