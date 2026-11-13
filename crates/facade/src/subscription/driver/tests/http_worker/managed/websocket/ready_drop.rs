//! A successful raw context future can fail destruction inside type erasure.

use std::process::Command;

use super::*;

const CHILD: &str = "CHIGIRI_MANAGED_WS_READY_CONTEXT_DROP_CHILD";
const TEST: &str = "subscription::driver::tests::http_worker::managed::websocket::ready_drop::successful_context_future_disposal_retains_the_original_managed_owners";

#[test]
fn successful_context_future_disposal_retains_the_original_managed_owners() {
    let Ok(case) = std::env::var(CHILD) else {
        for role in ["query", "mutation", "source", "event"] {
            for stop_first in [false, true] {
                let output = Command::new(std::env::current_exe().unwrap())
                    .args([TEST, "--exact", "--nocapture", "--test-threads=1"])
                    .env(CHILD, format!("{role}:{stop_first}"))
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "owned ready context {role}:{stop_first} child failed: {}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
        return;
    };
    let (role, stop_first) = case.split_once(':').unwrap();
    let scenario = match role {
        "query" => Scenario::Query,
        "mutation" => Scenario::Mutation,
        "source" => Scenario::SourceContext,
        "event" => Scenario::SourceEvent,
        _ => unreachable!(),
    };
    let probe = State::new(if scenario.event() {
        vec![SourceStep::Event(7)]
    } else {
        Vec::new()
    });
    let release = Release(probe.block_scalar());
    let errors = Arc::new(AtomicUsize::new(0));
    let schema = ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(probe.clone()))
        .build()
        .unwrap();
    let endpoint = Endpoint::builder(schema)
        .context_factory(ReadyDropFactory(
            Factory(probe.clone()),
            probe.clone(),
            scenario,
        ))
        .response_hook(Hook(probe.clone()))
        .inject(|components| {
            components
                .observability()
                .error_observer(Errors(errors.clone()));
        })
        .build()
        .unwrap();
    run(&runtime(), async {
        let managed = managed_socket::open(&endpoint).await;
        managed.peer.subscribe("ready", scenario.document());
        until(
            &probe.counts.factory_calls,
            1 + usize::from(scenario.event()),
        )
        .await;
        assert!(release.0.wait_entered(Duration::from_secs(2)));
        let hook = if stop_first == "true" {
            let mut hook = endpoint.shutdown_on(async {}, ShutdownOptions::graceful());
            assert!(
                poll_fn(|cx| Poll::Ready(hook.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
            Some(hook)
        } else {
            None
        };
        release.0.release();
        until(&errors, 1).await;
        hook.unwrap_or_else(|| endpoint.shutdown_on(async {}, ShutdownOptions::graceful()))
            .await;
        assert_eq!(
            endpoint.shutdown_outcome(),
            Some(ShutdownOutcome::InfrastructureFailure)
        );
        assert_eq!(count(&errors), 1);
        assert_eq!(count(&probe.counts.factory_future_drops), 1);
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
            1 + usize::from(scenario.event())
        );
        assert_eq!(count(&probe.counts.completion_calls), 0);
        assert_eq!(count(&probe.counts.nested_calls), 0);
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
                "the original concrete worker must reach native JoinReady; error={error}, {}",
                crate::subscription::worker::process_diagnostic_for_test()
            )
        });
        assert!(
            crate::subscription::worker::process_poisoned_for_test(),
            "successful context future disposal released the original physical record: {}",
            crate::subscription::worker::process_diagnostic_for_test()
        );
        assert_eq!(endpoint.subscription_connection_counts_for_test().0, 1);
        assert_eq!(
            endpoint.subscription_physical_counts_for_test().0,
            usize::from(scenario.subscription())
        );
        if !matches!(scenario, Scenario::SourceContext) {
            assert!(endpoint.subscription_connection_counts_for_test().1 > 0);
        }
        for frame in managed.peer.messages() {
            assert!(!frame.contains("private"));
            assert!(!frame.contains(r#""type":"next""#));
        }
    });
}

struct ReadyDropFactory(Factory, Arc<State>, Scenario);

pub(in crate::subscription::driver::tests::http_worker::managed) fn source_factory(
    probe: Arc<State>,
) -> impl ContextFactory<AppContext, HookState> {
    ReadyDropFactory(Factory(probe.clone()), probe, Scenario::SourceContext)
}

impl ContextFactory<AppContext, HookState> for ReadyDropFactory {
    fn create<'a>(
        &'a self,
        request: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<AppContext, HookState>, PrepareError>> {
        let selected =
            matches!(request.phase(), RequestPhase::SubscriptionEvent { .. }) == self.2.event();
        let future = self.0.create(request);
        if selected {
            Box::pin(ReadyDrop {
                future,
                probe: self.1.clone(),
                entered: false,
                ready: false,
            })
        } else {
            future
        }
    }
}

struct ReadyDrop<'a> {
    future: BoxFuture<'a, Result<PreparedContext<AppContext, HookState>, PrepareError>>,
    probe: Arc<State>,
    entered: bool,
    ready: bool,
}
impl Future for ReadyDrop<'_> {
    type Output = Result<PreparedContext<AppContext, HookState>, PrepareError>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if !self.entered {
            self.probe.wait_in_scalar();
            self.entered = true;
        }
        let result = self.future.as_mut().poll(cx);
        self.ready = result.is_ready();
        result
    }
}
impl Drop for ReadyDrop<'_> {
    fn drop(&mut self) {
        self.probe
            .counts
            .factory_future_drops
            .fetch_add(1, Ordering::SeqCst);
        assert!(
            self.ready,
            "the fixture must first return a prepared context"
        );
        panic!("private ready context future destruction failure");
    }
}
