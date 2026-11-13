//! Managed source and finite operation panics must survive outer cancellation.

mod authorization;
mod opening;
mod poison;
pub(super) mod ready_drop;
mod source_drop;
mod timeout;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use chigiri_http::{ContextFactory, PrepareError, PreparedContext, RequestContext, RequestPhase};
use chigiri_runtime::{ErrorEvent, ErrorObserver, ExecutableSchema};

use super::super::super::descriptor::SCHEMA;
use super::super::super::dispatch::Dispatch;
use super::super::super::harness::{Factory, Hook, managed_socket};
use super::super::super::state::{
    EventMode, FactoryMode, HookMode, HookState, ScalarGate, SetupMode,
};
use super::*;

struct Errors(Arc<AtomicUsize>);
impl ErrorObserver for Errors {
    fn observe_error(&self, _: ErrorEvent<'_>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct Release(Arc<ScalarGate>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}

#[derive(Clone, Copy, Debug)]
enum Scenario {
    Query,
    Mutation,
    SourceContext,
    SourceEvent,
}

impl Scenario {
    fn document(self) -> &'static str {
        match self {
            Self::Query => "query { ping }",
            Self::Mutation => "mutation { ping }",
            Self::SourceContext | Self::SourceEvent => {
                "subscription { events(limit: 2) { value } }"
            }
        }
    }

    fn event(self) -> bool {
        matches!(self, Self::SourceEvent)
    }
    fn subscription(self) -> bool {
        matches!(self, Self::SourceContext | Self::SourceEvent)
    }
}

#[derive(Clone, Copy, Debug)]
enum Failure {
    Panic,
    Payload,
    FutureDrop,
}

impl Failure {
    fn poisoned(self) -> bool {
        !matches!(self, Self::Panic)
    }
}

struct DroppingFuture {
    probe: Arc<State>,
    entered: bool,
}
impl Future for DroppingFuture {
    type Output = Result<PreparedContext<AppContext, HookState>, PrepareError>;
    fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        if !self.entered {
            self.probe.wait_in_scalar();
            self.entered = true;
        }
        Poll::Pending
    }
}
impl Drop for DroppingFuture {
    fn drop(&mut self) {
        self.probe
            .counts
            .factory_future_drops
            .fetch_add(1, Ordering::SeqCst);
        panic!("private WebSocket context future destruction failure");
    }
}

struct Payload;
impl Drop for Payload {
    fn drop(&mut self) {
        panic!("private WebSocket panic payload destruction failure");
    }
}

struct GatedFactory(Factory, Arc<State>, Scenario, Failure);
impl ContextFactory<AppContext, HookState> for GatedFactory {
    fn create<'a>(
        &'a self,
        request: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<AppContext, HookState>, PrepareError>> {
        let pause =
            matches!(request.phase(), RequestPhase::SubscriptionEvent { .. }) == self.2.event();
        let work = self.0.create(request);
        if pause && matches!(self.3, Failure::FutureDrop) {
            drop(work);
            return Box::pin(DroppingFuture {
                probe: self.1.clone(),
                entered: false,
            });
        }
        Box::pin(async move {
            if pause {
                self.1.wait_in_scalar();
                if matches!(self.3, Failure::Payload) {
                    std::panic::panic_any(Payload);
                }
            }
            work.await
        })
    }
}

#[test]
fn managed_active_panics_are_infrastructure_failure_for_source_query_and_mutation() {
    let runtime = runtime();
    for scenario in [
        Scenario::Query,
        Scenario::Mutation,
        Scenario::SourceContext,
        Scenario::SourceEvent,
    ] {
        for stop_first in [false, true] {
            exercise(&runtime, scenario, stop_first, Failure::Panic);
        }
    }
}

fn exercise(
    runtime: &tokio::runtime::Runtime,
    scenario: Scenario,
    stop_first: bool,
    failure: Failure,
) {
    let probe = State::modes(
        if scenario.event() {
            vec![SourceStep::Event(7)]
        } else {
            Vec::new()
        },
        SetupMode::Ready,
        EventMode::Object,
        if scenario.event() {
            FactoryMode::Ready
        } else {
            FactoryMode::Panic
        },
        if scenario.event() {
            FactoryMode::Panic
        } else {
            FactoryMode::Ready
        },
        HookMode::Ready,
    );
    let gate = probe.block_scalar();
    let release = Release(gate);
    let errors = Arc::new(AtomicUsize::new(0));
    let schema = ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(probe.clone()))
        .build()
        .unwrap();
    let endpoint = Endpoint::builder(schema)
        .context_factory(GatedFactory(
            Factory(probe.clone()),
            probe.clone(),
            scenario,
            failure,
        ))
        .response_hook(Hook(probe.clone()))
        .inject(|components| {
            components
                .observability()
                .error_observer(Errors(errors.clone()));
        })
        .build()
        .unwrap();
    run(runtime, async {
        let managed = managed_socket::open(&endpoint).await;
        managed.peer.subscribe("active", scenario.document());
        until(
            &probe.counts.factory_calls,
            1 + usize::from(scenario.event()),
        )
        .await;
        assert!(release.0.wait_entered(Duration::from_secs(2)));
        let shutdown = if stop_first {
            let options = if matches!(failure, Failure::FutureDrop) {
                ShutdownOptions::graceful_for(Duration::from_millis(20)).unwrap()
            } else {
                ShutdownOptions::graceful()
            };
            let mut hook = endpoint.shutdown_on(async {}, options);
            assert!(
                poll_fn(|cx| std::task::Poll::Ready(hook.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
            Some(hook)
        } else {
            None
        };
        release.0.release();
        until(&errors, 1).await;
        shutdown
            .unwrap_or_else(|| endpoint.shutdown_on(async {}, ShutdownOptions::graceful()))
            .await;
        let expected = if matches!(failure, Failure::FutureDrop)
            && !matches!(scenario, Scenario::SourceContext)
        {
            ShutdownOutcome::Expired
        } else {
            ShutdownOutcome::InfrastructureFailure
        };
        assert_eq!(endpoint.shutdown_outcome(), Some(expected));
        assert_eq!(
            count(&errors),
            1,
            "{scenario:?}, stop_first={stop_first}, {failure:?}"
        );
        assert_eq!(
            count(&probe.counts.setup_calls),
            usize::from(scenario.event())
        );
        assert_eq!(
            count(&probe.counts.source_drops),
            usize::from(scenario.event())
        );
        assert_eq!(
            count(&probe.counts.completion_drops),
            usize::from(scenario.event())
        );
        assert_eq!(count(&probe.counts.completion_calls), 0);
        assert_eq!(count(&probe.counts.hook_calls), 0);
        assert!(managed.service.request().is_err());
        if matches!(failure, Failure::FutureDrop) {
            assert_eq!(count(&probe.counts.factory_future_drops), 1);
        }
        if failure.poisoned() {
            let control = endpoint.managed_control();
            tokio::time::timeout(Duration::from_secs(2), async {
                while !crate::subscription::worker::process_poisoned_for_test() {
                    if !crate::subscription::worker::managed_supervisor_retained_for_test(&control)
                        && endpoint.subscription_connection_counts_for_test().0 == 0
                    {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "actual native JoinReady must select the poisoned record; error={error}, {}",
                    crate::subscription::worker::process_diagnostic_for_test()
                )
            });
            assert!(
                crate::subscription::worker::process_poisoned_for_test(),
                "failed disposal released the original physical record: {}",
                crate::subscription::worker::process_diagnostic_for_test()
            );
        }
        if failure.poisoned() {
            // An irreversible record remains reachable under recovery. The
            // native hook reports failure; it does not join process recovery.
            let (connections, bytes) = endpoint.subscription_connection_counts_for_test();
            assert_eq!(
                connections, 1,
                "original active operation retains its connection owner"
            );
            assert_eq!(
                endpoint.subscription_physical_counts_for_test().0,
                usize::from(scenario.subscription())
            );
            if !matches!(scenario, Scenario::SourceContext) {
                assert!(
                    bytes > 0,
                    "the original finite/event reservation stays charged"
                );
            }
        } else {
            let control = endpoint.managed_control();
            tokio::time::timeout(Duration::from_secs(2), async {
                while crate::subscription::worker::managed_supervisor_retained_for_test(&control) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("recoverable failed supervision must physically join");
            assert_eq!(endpoint.subscription_pending_probe_for_test()(), (0, 0));
            assert_eq!(endpoint.subscription_connection_counts_for_test(), (0, 0));
        }
        for frame in managed.peer.messages() {
            assert!(!frame.contains("private"));
            assert!(!frame.contains("fixture context panic"));
            assert!(!frame.contains(r#""type":"next""#));
        }
    });
}

#[test]
fn managed_source_failure_is_visible_when_its_setup_port_closes() {
    use crate::subscription::driver::tests::websocket_worker::support::{NEVER_DEADLINES, worker};
    use crate::subscription::worker::websocket::{
        WebSocketSubscriptionSetupOutcome, WebSocketValidationOutcome,
    };
    use std::sync::atomic::AtomicU8;

    let runtime = runtime();
    let probe = State::modes(
        Vec::new(),
        SetupMode::Ready,
        EventMode::Object,
        FactoryMode::Panic,
        FactoryMode::Ready,
        HookMode::Ready,
    );
    let release = Release(probe.block_scalar());
    let errors = Arc::new(AtomicUsize::new(0));
    let schema = ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(probe.clone()))
        .build()
        .unwrap();
    let endpoint = Endpoint::builder(schema)
        .context_factory(GatedFactory(
            Factory(probe.clone()),
            probe.clone(),
            Scenario::SourceContext,
            Failure::Panic,
        ))
        .response_hook(Hook(probe.clone()))
        .inject(|components| {
            components
                .observability()
                .error_observer(Errors(errors.clone()));
        })
        .build()
        .unwrap();
    run(&runtime, async {
        let service = endpoint.__managed_service().unwrap();
        let state = endpoint.managed_websocket_state();
        // Service activation owns the registered worker runtime.
        let connection = state.subscriptions.resources.open_connection().unwrap();
        let validation = worker(
            &runtime,
            &state,
            chigiri_runtime::GraphQlRequest::new(Scenario::SourceContext.document()),
            None,
            connection.begin(Arc::from("failure-order")).unwrap(),
            &NEVER_DEADLINES,
        );
        let WebSocketValidationOutcome::Prepared(prepared) = validation.await else {
            panic!("source validation did not prepare");
        };
        let mut setup = prepared
            .activate_subscription()
            .unwrap_or_else(|_| panic!("source admission failed"));
        assert!(release.0.wait_entered(Duration::from_secs(2)));
        // Observe synchronously at the actual close wake. No assertion or recursive poll may
        // run on the worker's unwind stack. 0 = no wake, 1 = not failed, 2 = failed.
        let observed = Arc::new(AtomicU8::new(0));
        let seen = observed.clone();
        setup.observe_setup_close_for_test(move |failed| {
            let _ = seen.compare_exchange(
                0,
                if failed { 2 } else { 1 },
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
        });
        release.0.release();
        tokio::time::timeout(Duration::from_secs(2), async {
            while observed.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the owned setup port must close");
        assert_eq!(
            observed.load(Ordering::SeqCst),
            2,
            "port closure exposed an unclassified worker failure"
        );
        assert!(matches!(
            setup.await,
            WebSocketSubscriptionSetupOutcome::Terminal(_)
        ));
        drop(connection.revoke().unwrap());
        endpoint
            .shutdown_on(async {}, ShutdownOptions::graceful())
            .await;
        assert_eq!(
            endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::InfrastructureFailure)
        );
        let control = endpoint.managed_control();
        tokio::time::timeout(Duration::from_secs(2), async {
            while crate::subscription::worker::managed_supervisor_retained_for_test(&control) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the original managed supervisor must physically join");
        assert_eq!(endpoint.subscription_pending_probe_for_test()(), (0, 0));
        assert_eq!(endpoint.subscription_connection_counts_for_test(), (0, 0));
        assert_eq!(endpoint.subscription_physical_counts_for_test(), (0, 0));
        assert_eq!(count(&errors), 1);
        assert_eq!(count(&probe.counts.factory_calls), 1);
        assert_eq!(count(&probe.counts.setup_calls), 0);
        assert_eq!(count(&probe.counts.completion_calls), 0);
        assert!(service.request().is_err());
    });
}
