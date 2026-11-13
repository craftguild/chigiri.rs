//! Demand one owned item, then prepare and execute exactly one fresh event.

#[cfg(feature = "subscription-worker")]
mod monitored;

use chigiri_http::{BoxFuture, DeadlineFactory, RequestPhase};
use chigiri_runtime::__private::{
    SubscriptionEventResult, SubscriptionSourceState, SubscriptionTerminalResponse,
};
use chigiri_runtime::instrumentation::{CapturedInstrumentation, InstrumentationKind};
use chigiri_runtime::{
    GraphQlError, GraphQlResponse, LifecyclePhase, LifecycleResult, RequestTelemetry,
};

use super::{EndpointSubscription, SubscriptionOutput};
use crate::subscription::preparation::{ContextPreparation, prepare_context};
use crate::transport::run;

const MAX_EVENT_SEQUENCE: u64 = 9_007_199_254_740_991;

impl<C: Send + Sync + 'static> EndpointSubscription<C> {
    /// Demands and executes one event, creating no context while its source is idle.
    ///
    /// Dropping this future revokes the source even before its first poll. A
    /// normal event restores ownership only after its context and state release.
    /// The common delivery owner must acquire its single-event byte reservation
    /// before requesting another item through this internal bridge seam.
    pub fn next<'a>(
        &'a mut self,
        deadlines: &'a dyn DeadlineFactory,
    ) -> BoxFuture<'a, SubscriptionOutput> {
        let session = self.session.take();
        Box::pin(async move {
            let Some(mut session) = session else {
                return SubscriptionOutput::Cancelled;
            };
            match session
                .next_source(self.request.request().request_telemetry())
                .await
            {
                SubscriptionSourceState::Complete => {
                    return self.finish_source_output(session, SubscriptionOutput::Complete);
                }
                SubscriptionSourceState::Cancelled => {
                    return self.finish_source_output(session, SubscriptionOutput::Cancelled);
                }
                SubscriptionSourceState::Terminal(response) => {
                    return self
                        .finish_source_output(session, SubscriptionOutput::Terminal(response));
                }
                SubscriptionSourceState::EventReady => {}
            }
            let Some(sequence) = self
                .sequence
                .checked_add(1)
                .filter(|value| *value <= MAX_EVENT_SEQUENCE)
            else {
                return self.finish_source_output(
                    session,
                    terminal(
                        "Subscription event sequence exhausted",
                        "SUBSCRIPTION_LIMIT_EXCEEDED",
                    ),
                );
            };
            let captured = self.event_capture();
            let handoff = captured
                .as_ref()
                .map(|captured| captured.handoff_scope(InstrumentationKind::SubscriptionEvent));
            let payload_scope = handoff.clone();
            let work = async move {
                session.bind_event_instrumentation(payload_scope);
                let deadline = deadlines.sleep(self.state.subscriptions.limits().event_timeout);
                let telemetry = self.state.schema.request_telemetry_with_capture(
                    self.request.trace_id().cloned(),
                    captured.as_ref(),
                    InstrumentationKind::SubscriptionEvent,
                );
                let scope = telemetry.instrumentation_scope().cloned();
                session.bind_event_instrumentation(scope.clone());
                let mut outcome = LifecycleResult::Failed;
                let work = self.execute_local_event(
                    session,
                    sequence,
                    deadlines,
                    deadline,
                    &telemetry,
                    &mut outcome,
                );
                let output = match &scope {
                    Some(scope) => scope.instrument(work).await,
                    None => work.await,
                };
                if let Some(scope) = scope {
                    scope.finish(outcome);
                }
                output
            };
            match handoff {
                Some(scope) => scope.instrument(work).await,
                None => work.await,
            }
        })
    }

    fn event_capture(&self) -> Option<CapturedInstrumentation> {
        self.request
            .request()
            .request_telemetry()
            .and_then(RequestTelemetry::propagation_context)
            .and_then(|values| self.state.schema.instrumentation_with_values(values))
    }

    async fn execute_local_event(
        &mut self,
        mut session: chigiri_runtime::__private::SubscriptionSession<C>,
        sequence: u64,
        deadlines: &dyn DeadlineFactory,
        mut deadline: BoxFuture<'_, ()>,
        telemetry: &RequestTelemetry,
        outcome: &mut LifecycleResult,
    ) -> SubscriptionOutput {
        let fresh = match prepare_context(
            &self.state.schema,
            &self.state.context_factory,
            &self.request,
            RequestPhase::SubscriptionEvent {
                transport: self.request.transport(),
                sequence,
            },
            deadlines,
            self.state.settings.context_timeout,
            deadline.as_mut(),
            Some(telemetry),
        )
        .await
        {
            ContextPreparation::Ready(fresh) => fresh,
            ContextPreparation::Rejected { rejection, .. } => {
                *outcome = LifecycleResult::Rejected;
                return self.finish_source_output(
                    session,
                    SubscriptionOutput::Terminal(SubscriptionTerminalResponse::execution(
                        GraphQlResponse::request_error(rejection.error().clone()),
                    )),
                );
            }
            ContextPreparation::Internal { error, telemetry } => {
                *outcome = LifecycleResult::Failed;
                let response = self.internal_failure(&error, &telemetry);
                return self.finish_source_output(
                    session,
                    SubscriptionOutput::Terminal(SubscriptionTerminalResponse::execution(response)),
                );
            }
            ContextPreparation::TimedOut { .. } => {
                *outcome = LifecycleResult::TimedOut;
                return self.finish_source_output(
                    session,
                    SubscriptionOutput::Terminal(SubscriptionTerminalResponse::execution(
                        crate::timeout::request_timeout_response(),
                    )),
                );
            }
        };
        let result = run(
            deadlines,
            self.state.settings.execution_timeout,
            deadline.as_mut(),
            session.execute_event(
                fresh.execution_context(),
                Some(fresh.telemetry()),
                Some(deadlines),
            ),
        )
        .await;
        drop(fresh);
        match result {
            Ok(SubscriptionEventResult::Event(response)) => {
                *outcome = response_result(&response);
                self.sequence = sequence;
                self.session = Some(session);
                SubscriptionOutput::Event(response)
            }
            Ok(SubscriptionEventResult::Terminal(response)) => {
                *outcome = LifecycleResult::Failed;
                self.finish_source_output(session, SubscriptionOutput::Terminal(response))
            }
            Ok(SubscriptionEventResult::Cancelled) => {
                *outcome = LifecycleResult::Cancelled;
                self.finish_source_output(session, SubscriptionOutput::Cancelled)
            }
            Err(_) => {
                *outcome = LifecycleResult::TimedOut;
                self.finish_source_output(
                    session,
                    SubscriptionOutput::Terminal(SubscriptionTerminalResponse::execution(
                        crate::timeout::request_timeout_response(),
                    )),
                )
            }
        }
    }

    fn finish_source_output(
        &mut self,
        session: chigiri_runtime::__private::SubscriptionSession<C>,
        output: SubscriptionOutput,
    ) -> SubscriptionOutput {
        if session.host_observation_receipt().is_some() {
            self.session = Some(session);
        }
        output
    }

    fn internal_failure(
        &self,
        source: &(dyn std::error::Error + Send + Sync + 'static),
        telemetry: &RequestTelemetry,
    ) -> GraphQlResponse {
        let id = self.state.schema.observe_internal_error(
            LifecyclePhase::ContextConstruction,
            "INTERNAL_SERVER_ERROR",
            source,
            Some(telemetry),
            "subscription-event-context",
        );
        let mut error = GraphQlError::new("Internal server error").code("INTERNAL_SERVER_ERROR");
        if let Some(id) = id {
            error = error.extension("errorId", id.as_str());
        }
        GraphQlResponse::request_error(error)
    }
}

fn terminal(message: &'static str, code: &'static str) -> SubscriptionOutput {
    SubscriptionOutput::Terminal(SubscriptionTerminalResponse::execution(
        GraphQlResponse::request_error(GraphQlError::new(message).code(code)),
    ))
}

fn response_result(response: &GraphQlResponse) -> LifecycleResult {
    if response.data.is_none() && !response.errors.is_empty() {
        LifecycleResult::Rejected
    } else {
        LifecycleResult::Succeeded
    }
}
