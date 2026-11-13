use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

use chigiri_http::{
    BoxFuture, ContextFactory, PrepareError, PreparedContext, RequestContext, RequestPhase,
    ResponseCompletion, ResponseCompletionOutcome, ResponseParts,
};

use super::super::super::state::{AppContext, ContextKind, HookState, State};
use crate::subscription::execution_state::SubscriptionExecutionState;
use crate::transport::{EndpointState, erase_factory};

pub(super) fn install(endpoint: &mut Arc<EndpointState<AppContext>>, state: Arc<State>) {
    let endpoint = Arc::get_mut(endpoint).expect("one supervision endpoint owner");
    endpoint.context_factory = erase_factory(SequencedFactory {
        state,
        event_calls: AtomicUsize::new(0),
    });
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

struct SequencedFactory {
    state: Arc<State>,
    event_calls: AtomicUsize,
}

impl ContextFactory<AppContext, HookState> for SequencedFactory {
    fn create<'a>(
        &'a self,
        request: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<AppContext, HookState>, PrepareError>> {
        self.state
            .counts
            .factory_calls
            .fetch_add(1, Ordering::SeqCst);
        let phase = request.phase();
        self.state.phases.lock().unwrap().push(phase);
        self.state.phase_request_ids.lock().unwrap().push(
            request
                .transport()
                .request_telemetry()
                .and_then(|telemetry| telemetry.request_id())
                .map(|id| id.as_str().to_owned()),
        );
        let kind = match phase {
            RequestPhase::SubscriptionSetup { .. } => ContextKind::Setup,
            RequestPhase::SubscriptionEvent { .. } => {
                if self.event_calls.fetch_add(1, Ordering::SeqCst) == 1 {
                    return Box::pin(PendingSecondEvent(Arc::clone(&self.state)));
                }
                ContextKind::Event
            }
            _ => panic!("subscription fixture received a finite request phase"),
        };
        let state = Arc::clone(&self.state);
        let generation = state.counts.factory_calls.load(Ordering::SeqCst);
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
            .with_response_completion(Completion(state)))
        })
    }
}

struct PendingSecondEvent(Arc<State>);

impl Future for PendingSecondEvent {
    type Output = Result<PreparedContext<AppContext, HookState>, PrepareError>;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

impl Drop for PendingSecondEvent {
    fn drop(&mut self) {
        self.0
            .counts
            .factory_future_drops
            .fetch_add(1, Ordering::SeqCst);
    }
}

struct Completion(Arc<State>);

impl ResponseCompletion for Completion {
    fn complete(
        &self,
        response: ResponseParts,
        outcome: ResponseCompletionOutcome,
    ) -> ResponseParts {
        self.0
            .counts
            .completion_calls
            .fetch_add(1, Ordering::SeqCst);
        self.0.completion_outcomes.lock().unwrap().push(outcome);
        response
    }
}

impl Drop for Completion {
    fn drop(&mut self) {
        self.0
            .counts
            .completion_drops
            .fetch_add(1, Ordering::SeqCst);
    }
}
