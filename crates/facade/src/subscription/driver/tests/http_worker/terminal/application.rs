//! Application fixtures for terminal callback and source ownership.

mod source;
pub(in crate::subscription::driver::tests::http_worker) use source::panicking_source_schema;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use super::super::super::state::{AppContext, ContextKind, HookState, State};
use super::support::ManualDeadline;
use crate::subscription::execution_state::SubscriptionExecutionState;
use crate::transport::{EndpointState, erase_factory, erase_hook};
use chigiri_http::{
    BoxFuture, ContextFactory, HookError, PrepareError, PreparedContext, RequestContext,
    ResponseCompletion, ResponseCompletionOutcome, ResponseHeaders, ResponseHook,
    ResponseHookContext, ResponseParts,
};

pub(super) use source::{SourceBehavior, install_source};

#[derive(Clone)]
pub(super) enum HookBehavior {
    Trigger(&'static ManualDeadline),
    Block(Arc<BlockingGate>),
}

#[derive(Clone)]
pub(super) enum CompletionBehavior {
    Normal,
    Oversize,
    Panic,
    NestedPanic(Arc<AtomicUsize>),
    Block(Arc<BlockingGate>),
}

pub(super) struct CompletionProbe {
    behavior: CompletionBehavior,
    overall: Option<&'static ManualDeadline>,
    trigger_overall: bool,
    calls: AtomicUsize,
    drops: AtomicUsize,
    source_drops_at_call: Mutex<Vec<usize>>,
    overall_drops_at_call: Mutex<Vec<usize>>,
    outcomes: Mutex<Vec<ResponseCompletionOutcome>>,
}

impl CompletionProbe {
    pub(super) fn new(
        behavior: CompletionBehavior,
        overall: Option<&'static ManualDeadline>,
        trigger_overall: bool,
    ) -> Arc<Self> {
        Arc::new(Self {
            behavior,
            overall,
            trigger_overall,
            calls: AtomicUsize::new(0),
            drops: AtomicUsize::new(0),
            source_drops_at_call: Mutex::new(Vec::new()),
            overall_drops_at_call: Mutex::new(Vec::new()),
            outcomes: Mutex::new(Vec::new()),
        })
    }

    pub(super) fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    pub(super) fn outcomes(&self) -> Vec<ResponseCompletionOutcome> {
        self.outcomes.lock().unwrap().clone()
    }

    pub(super) fn source_drops_at_call(&self) -> Vec<usize> {
        self.source_drops_at_call.lock().unwrap().clone()
    }

    pub(super) fn overall_drops_at_call(&self) -> Vec<usize> {
        self.overall_drops_at_call.lock().unwrap().clone()
    }
}

struct TerminalFactory {
    state: Arc<State>,
    completion: Arc<CompletionProbe>,
}

impl ContextFactory<AppContext, HookState> for TerminalFactory {
    fn create<'a>(
        &'a self,
        request: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<AppContext, HookState>, PrepareError>> {
        self.state
            .counts
            .factory_calls
            .fetch_add(1, Ordering::SeqCst);
        let kind = match request.phase() {
            chigiri_http::RequestPhase::SubscriptionSetup { .. } => ContextKind::Setup,
            chigiri_http::RequestPhase::SubscriptionEvent { .. } => ContextKind::Event,
            _ => panic!("terminal fixture received a non-subscription phase"),
        };
        let generation = self.state.counts.factory_calls.load(Ordering::SeqCst);
        let state = Arc::clone(&self.state);
        let completion = Arc::clone(&self.completion);
        Box::pin(async move {
            Ok(PreparedContext::new(
                AppContext {
                    generation,
                    kind,
                    state: Arc::clone(&state),
                },
                HookState {
                    kind,
                    state: Arc::clone(&state),
                },
            )
            .with_response_completion(TerminalCompletion { state, completion }))
        })
    }
}

struct TerminalCompletion {
    state: Arc<State>,
    completion: Arc<CompletionProbe>,
}

impl ResponseCompletion for TerminalCompletion {
    fn complete(
        &self,
        mut response: ResponseParts,
        outcome: ResponseCompletionOutcome,
    ) -> ResponseParts {
        self.completion.calls.fetch_add(1, Ordering::SeqCst);
        self.completion
            .source_drops_at_call
            .lock()
            .unwrap()
            .push(self.state.counts.source_drops.load(Ordering::SeqCst));
        self.completion.outcomes.lock().unwrap().push(outcome);
        if let Some(overall) = self.completion.overall {
            self.completion
                .overall_drops_at_call
                .lock()
                .unwrap()
                .push(overall.drops());
            if self.completion.trigger_overall {
                overall.trigger();
            }
        }
        match &self.completion.behavior {
            CompletionBehavior::Normal => response,
            CompletionBehavior::Oversize => {
                response.body = vec![b'x'; 65 * 1024];
                response
            }
            CompletionBehavior::Panic => panic!("terminal completion panic"),
            CompletionBehavior::NestedPanic(drops) => {
                std::panic::panic_any(PanicPayload(Arc::clone(drops)))
            }
            CompletionBehavior::Block(gate) => {
                gate.enter_and_wait();
                response
            }
        }
    }
}

impl Drop for TerminalCompletion {
    fn drop(&mut self) {
        self.completion.drops.fetch_add(1, Ordering::SeqCst);
    }
}

struct PanicPayload(Arc<AtomicUsize>);

impl Drop for PanicPayload {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("nested panic payload drop");
    }
}

pub(super) fn install_completion(
    endpoint: &mut Arc<EndpointState<AppContext>>,
    state: Arc<State>,
    completion: Arc<CompletionProbe>,
) {
    let endpoint = Arc::get_mut(endpoint).expect("one terminal endpoint owner");
    endpoint.context_factory = erase_factory(TerminalFactory { state, completion });
    endpoint.subscription_execution = Arc::new(SubscriptionExecutionState {
        #[cfg(feature = "subscription-worker")]
        managed: std::sync::OnceLock::new(),
        schema: endpoint.schema.clone(),
        context_factory: Arc::clone(&endpoint.context_factory),
        response_hook: endpoint.response_hook.clone(),
        settings: endpoint.settings,
        subscriptions: endpoint.subscriptions.settings,
    });
}

struct TerminalHook {
    state: Arc<State>,
    behavior: HookBehavior,
}

impl ResponseHook<HookState> for TerminalHook {
    fn call<'a>(
        &'a self,
        _: ResponseHookContext<'a, HookState>,
    ) -> BoxFuture<'a, Result<ResponseHeaders, HookError>> {
        self.state.counts.hook_calls.fetch_add(1, Ordering::SeqCst);
        let behavior = self.behavior.clone();
        Box::pin(async move {
            match behavior {
                HookBehavior::Trigger(deadline) => deadline.trigger(),
                HookBehavior::Block(gate) => gate.enter_and_wait(),
            }
            let mut headers = ResponseHeaders::new();
            headers.append("x-subscription", "terminal").unwrap();
            Ok(headers)
        })
    }
}

pub(super) fn install_hook(
    endpoint: &mut Arc<EndpointState<AppContext>>,
    state: Arc<State>,
    behavior: HookBehavior,
) {
    let endpoint = Arc::get_mut(endpoint).expect("one terminal endpoint owner");
    endpoint.response_hook = Some(erase_hook(TerminalHook { state, behavior }));
    endpoint.subscription_execution = Arc::new(SubscriptionExecutionState {
        #[cfg(feature = "subscription-worker")]
        managed: std::sync::OnceLock::new(),
        schema: endpoint.schema.clone(),
        context_factory: Arc::clone(&endpoint.context_factory),
        response_hook: endpoint.response_hook.clone(),
        settings: endpoint.settings,
        subscriptions: endpoint.subscriptions.settings,
    });
}

pub(super) struct BlockingGate {
    state: Mutex<GateState>,
    signal: Condvar,
}

struct GateState {
    entered: bool,
    released: bool,
}

impl BlockingGate {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(GateState {
                entered: false,
                released: false,
            }),
            signal: Condvar::new(),
        })
    }

    fn enter_and_wait(&self) {
        let mut state = self.state.lock().unwrap();
        state.entered = true;
        self.signal.notify_all();
        while !state.released {
            state = self.signal.wait(state).unwrap();
        }
    }

    pub(super) fn wait_entered(&self) {
        let state = self.state.lock().unwrap();
        let (state, timeout) = self
            .signal
            .wait_timeout_while(state, Duration::from_secs(2), |state| !state.entered)
            .unwrap();
        drop(state);
        assert!(!timeout.timed_out(), "application gate was not entered");
    }

    pub(super) fn entered(&self) -> bool {
        self.state.lock().unwrap().entered
    }

    pub(super) fn release(&self) {
        self.state.lock().unwrap().released = true;
        self.signal.notify_all();
    }
}

impl Drop for BlockingGate {
    fn drop(&mut self) {
        self.state.get_mut().unwrap().released = true;
        self.signal.notify_all();
    }
}
