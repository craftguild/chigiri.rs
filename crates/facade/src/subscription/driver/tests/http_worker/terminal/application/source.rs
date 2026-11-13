//! Subscription source fixtures with adversarial destruction and event work.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::{Context, Poll};

use chigiri_http::BoxFuture;
use chigiri_runtime::__private::{
    ArgumentValues, ErasedSubscription, ErasedSubscriptionArguments, ErasedSubscriptionEvent,
    ResolvedObject, ResolvedValue, ResolverDispatch, ResolverSchemaFingerprint,
    SubscriptionResolverDispatch, descriptor_fingerprint, erase_subscription_source,
    invariant_error,
};
use chigiri_runtime::core::executable::CustomScalarRegistry;
use chigiri_runtime::{
    ExecutableSchema, FieldContext, FieldError, FieldResult, SubscriptionSource,
};
use futures_core::Stream;

use super::super::super::super::descriptor::SCHEMA;
use super::super::super::super::state::{AppContext, State};
use super::BlockingGate;
use crate::subscription::execution_state::SubscriptionExecutionState;
use crate::transport::EndpointState;

#[derive(Clone)]
pub(in crate::subscription::driver::tests::http_worker::terminal) enum SourceBehavior {
    PanicOnDrop,
    CompleteThenPanicOnDrop,
    BlockOnDrop(Arc<BlockingGate>),
    BlockingEvent(Arc<BlockingGate>),
}

pub(in crate::subscription::driver::tests::http_worker::terminal) fn install_source(
    endpoint: &mut Arc<EndpointState<AppContext>>,
    state: Arc<State>,
    behavior: SourceBehavior,
) {
    let schema =
        ExecutableSchema::builder_with_subscriptions(&SCHEMA, TerminalDispatch { state, behavior })
            .build()
            .unwrap();
    let endpoint = Arc::get_mut(endpoint).expect("one terminal endpoint owner");
    endpoint.schema = schema.clone();
    endpoint.subscription_execution = Arc::new(SubscriptionExecutionState {
        #[cfg(feature = "subscription-worker")]
        managed: std::sync::OnceLock::new(),
        schema,
        context_factory: Arc::clone(&endpoint.context_factory),
        response_hook: endpoint.response_hook.clone(),
        settings: endpoint.settings,
        subscriptions: endpoint.subscriptions.settings,
    });
}

struct TerminalDispatch {
    state: Arc<State>,
    behavior: SourceBehavior,
}

pub(in crate::subscription::driver::tests::http_worker) fn panicking_source_schema(
    state: Arc<State>,
    completes: bool,
) -> ExecutableSchema<AppContext> {
    ExecutableSchema::builder_with_subscriptions(
        &SCHEMA,
        TerminalDispatch {
            state,
            behavior: if completes {
                SourceBehavior::CompleteThenPanicOnDrop
            } else {
                SourceBehavior::PanicOnDrop
            },
        },
    )
    .build()
    .unwrap()
}

impl ResolverDispatch<AppContext> for TerminalDispatch {
    fn schema_fingerprint(&self) -> ResolverSchemaFingerprint {
        descriptor_fingerprint(&SCHEMA)
    }

    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}

    fn resolve<'a>(
        &'a self,
        _: &'a AppContext,
        _: &'a str,
        _: &'a str,
        _: Option<&'a ResolvedObject>,
        _: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async { Err(invariant_error("unused terminal ordinary resolver")) })
    }
}

impl SubscriptionResolverDispatch<AppContext> for TerminalDispatch {
    fn subscription_root(&self) -> Option<&'static str> {
        Some("Subscription")
    }

    fn subscribe<'a>(
        &'a self,
        _: FieldContext<'a, AppContext>,
        _: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ErasedSubscription>> {
        self.state.counts.setup_calls.fetch_add(1, Ordering::SeqCst);
        let stream = TerminalStream {
            state: Arc::clone(&self.state),
            behavior: self.behavior.clone(),
            emitted: false,
        };
        Box::pin(async move {
            Ok(erase_subscription_source(
                SubscriptionSource::new(stream),
                (),
                Arc::new(|error: TerminalSourceError| FieldError::internal(error)),
            ))
        })
    }

    fn resolve_event<'a>(
        &'a self,
        _: FieldContext<'a, AppContext>,
        _: &'a ErasedSubscriptionArguments,
        _: &'a ErasedSubscriptionEvent,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        let behavior = self.behavior.clone();
        Box::pin(async move {
            if let SourceBehavior::BlockingEvent(gate) = behavior {
                gate.enter_and_wait();
            }
            Err(invariant_error(
                "terminal event ends after the blocking gate",
            ))
        })
    }
}

struct TerminalStream {
    state: Arc<State>,
    behavior: SourceBehavior,
    emitted: bool,
}

impl Stream for TerminalStream {
    type Item = Result<i32, TerminalSourceError>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.state
            .counts
            .source_polls
            .fetch_add(1, Ordering::SeqCst);
        match self.behavior {
            SourceBehavior::CompleteThenPanicOnDrop => Poll::Ready(None),
            SourceBehavior::BlockingEvent(_) if !self.emitted => {
                self.emitted = true;
                Poll::Ready(Some(Ok(1)))
            }
            _ => Poll::Pending,
        }
    }
}

impl Drop for TerminalStream {
    fn drop(&mut self) {
        self.state
            .counts
            .source_drops
            .fetch_add(1, Ordering::SeqCst);
        match &self.behavior {
            SourceBehavior::PanicOnDrop | SourceBehavior::CompleteThenPanicOnDrop => {
                panic!("terminal source destructor panic")
            }
            SourceBehavior::BlockOnDrop(gate) => gate.enter_and_wait(),
            SourceBehavior::BlockingEvent(_) => {}
        }
    }
}

#[derive(Debug)]
struct TerminalSourceError;

impl std::fmt::Display for TerminalSourceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("terminal source error")
    }
}

impl std::error::Error for TerminalSourceError {}
