//! Cancellation-safe transitions between one source item and one event execution.

use chigiri_core::executable::InputCoercionBudget;
use chigiri_core::executor::{
    BoxFuture, FieldContext, ResolvedValue, SubscriptionEventDispatch, SubscriptionExecutionInput,
    execute_subscription_event,
};
use chigiri_core::{DeadlineFactory, FieldResult, RequestTelemetry};

use super::diagnostics::EventDiagnostics;
use super::{SessionState, SubscriptionEventResult, SubscriptionSession, SubscriptionSourceState};
use crate::instrumentation::instrument_future;
use crate::subscription::{
    ErasedSubscriptionArguments, ErasedSubscriptionEvent, SubscriptionResolverDispatch,
    SubscriptionTerminalResponse, SubscriptionTermination,
};

impl<C: Send + Sync + 'static> SubscriptionSession<C> {
    /// Demands at most one source item, leaving its event unexecuted.
    pub fn next_source<'a>(
        &'a mut self,
        telemetry: Option<&'a RequestTelemetry>,
    ) -> BoxFuture<'a, SubscriptionSourceState> {
        if self.state == SessionState::EventReady {
            return Box::pin(async { SubscriptionSourceState::EventReady });
        }
        let Some(mut source) = self.source.take() else {
            return Box::pin(async move {
                if self.state == SessionState::Complete {
                    SubscriptionSourceState::Complete
                } else {
                    SubscriptionSourceState::Cancelled
                }
            });
        };
        self.state = SessionState::Cancelled;
        instrument_future(self.source_instrumentation.clone(), async move {
            match std::future::poll_fn(|context| source.as_mut().poll_next(context)).await {
                Some(Ok(event)) => {
                    self.current_event = Some(event);
                    self.source = Some(source);
                    self.state = SessionState::EventReady;
                    SubscriptionSourceState::EventReady
                }
                Some(Err(error)) => {
                    self.state = SessionState::Failed;
                    self.retain_for_outside_cleanup(source);
                    SubscriptionSourceState::Terminal(SubscriptionTerminalResponse::execution(
                        self.source_terminal_response(error, telemetry),
                    ))
                }
                None => {
                    self.state = SessionState::Complete;
                    self.retain_for_outside_cleanup(source);
                    SubscriptionSourceState::Complete
                }
            }
        })
    }

    /// Executes one demanded event with a fresh context and fresh resource counters.
    pub fn execute_event<'a>(
        &'a mut self,
        context: &'a C,
        telemetry: Option<&'a RequestTelemetry>,
        deadlines: Option<&'a dyn DeadlineFactory>,
    ) -> BoxFuture<'a, SubscriptionEventResult> {
        let source = self.source.take();
        let event_scope = telemetry
            .and_then(RequestTelemetry::instrumentation_scope)
            .cloned();
        self.bind_event_instrumentation(event_scope.clone());
        let event = self.current_event.take();
        let previous = self.state;
        self.state = SessionState::Cancelled;
        instrument_future(event_scope, async move {
            if previous == SessionState::Cancelled
                || previous == SessionState::Complete
                || previous == SessionState::Failed
            {
                return SubscriptionEventResult::Cancelled;
            }
            let Some(source) = source else {
                self.state = SessionState::Failed;
                return SubscriptionEventResult::Terminal(SubscriptionTerminalResponse::internal(
                    self.terminal_response(
                        crate::private::invariant_error(
                            "event execution requires one demanded source item",
                        ),
                        telemetry,
                    ),
                ));
            };
            let Some(event) = event else {
                self.state = SessionState::Failed;
                self.retain_for_outside_cleanup(source);
                return SubscriptionEventResult::Terminal(SubscriptionTerminalResponse::internal(
                    self.terminal_response(
                        crate::private::invariant_error(
                            "event execution requires one demanded source item",
                        ),
                        telemetry,
                    ),
                ));
            };
            let Some(dispatch) = self.schema.inner.subscription.as_deref() else {
                self.state = SessionState::Failed;
                self.retain_for_outside_cleanup(source);
                return SubscriptionEventResult::Terminal(SubscriptionTerminalResponse::internal(
                    self.terminal_response(
                        crate::private::invariant_error("subscription event dispatch is absent"),
                        telemetry,
                    ),
                ));
            };
            let adapter = Event {
                dispatch,
                arguments: &self.arguments,
                event: &event,
            };
            let budget = self
                .schema
                .inner
                .resolvers
                .input_coercion_limits()
                .map(|(nodes, bytes)| InputCoercionBudget::new(nodes, bytes));
            let diagnostics = EventDiagnostics::new(self.schema.inner.diagnostics.clone());
            let input = SubscriptionExecutionInput::new(
                self.schema.inner.resolvers.as_ref(),
                &self.schema.inner.plugins,
                context,
                Some(&diagnostics),
                self.schema.inner.diagnostics.lifecycle(),
                telemetry,
                deadlines,
                budget.as_ref(),
            );
            let response = execute_subscription_event(input, &self.root, &adapter).await;
            let host_failed = dispatch.event_execution_failed(context);
            let termination = diagnostics.termination();
            if termination.is_some() || host_failed {
                self.state = SessionState::Failed;
                let errors = if response.errors.is_empty() {
                    vec![
                        chigiri_core::GraphQlError::new("Internal server error")
                            .code("INTERNAL_SERVER_ERROR"),
                    ]
                } else {
                    response.errors
                };
                let response = chigiri_core::GraphQlResponse {
                    data: None,
                    errors,
                    extensions: chigiri_core::Object::new(),
                };
                self.retain_for_outside_cleanup(source);
                return SubscriptionEventResult::Terminal(match termination {
                    Some(SubscriptionTermination::Internal) => {
                        SubscriptionTerminalResponse::internal(response)
                    }
                    _ => SubscriptionTerminalResponse::execution(response),
                });
            }
            self.source = Some(source);
            self.state = SessionState::Polling;
            SubscriptionEventResult::Event(response)
        })
    }
}

struct Event<'a, C> {
    dispatch: &'a dyn SubscriptionResolverDispatch<C>,
    arguments: &'a ErasedSubscriptionArguments,
    event: &'a ErasedSubscriptionEvent,
}

impl<C: 'static> SubscriptionEventDispatch<C> for Event<'_, C> {
    fn resolve_event<'a>(
        &'a self,
        context: FieldContext<'a, C>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.dispatch
            .resolve_event(context, self.arguments, self.event)
    }
}
