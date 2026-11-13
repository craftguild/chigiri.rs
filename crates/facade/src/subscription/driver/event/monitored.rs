//! Monitored event execution and the managed source-start fence.

use chigiri_http::RequestPhase;
use chigiri_runtime::__private::{
    SubscriptionEventResult, SubscriptionSession, SubscriptionSourceState,
    SubscriptionTerminalResponse,
};
use chigiri_runtime::instrumentation::InstrumentationKind;
use chigiri_runtime::{GraphQlResponse, LifecycleResult, RequestTelemetry};

use super::{EndpointSubscription, MAX_EVENT_SEQUENCE, SubscriptionOutput, terminal};
use crate::managed::EventActivity;
use crate::subscription::preparation::{ContextPreparation, prepare_context_monitored};
use crate::subscription::worker::{
    DeadlineArmer, DeadlineRole, SubscriptionWorkerRuntimeError, TaskOwnedFuture, TaskOwnedValue,
    guard_instrumented_work, run_monitored,
};

impl<C: Send + Sync + 'static> EndpointSubscription<C> {
    /// Runs one demanded event while the outer worker polls every exact timer.
    #[cfg(feature = "subscription-worker")]
    pub(crate) async fn next_monitored(
        &mut self,
        armer: &DeadlineArmer,
        activity: &EventActivity,
    ) -> Result<SubscriptionOutput, SubscriptionWorkerRuntimeError> {
        let Some(session) = self.session.take() else {
            return Ok(SubscriptionOutput::Cancelled);
        };
        let mut session = TaskOwnedValue::new(session, Some(activity.clone()));
        if activity.is_managed() {
            session.defer_source_cleanup();
        }
        let source = {
            // The inner future takes the source out of its session. Its
            // cancellation must retain disposal evidence before select returns.
            let next = TaskOwnedFuture::new(
                session.next_source(self.request.request().request_telemetry()),
                Some(activity.clone()),
            );
            tokio::select! {
                biased;
                _ = activity.source_stopped() => SubscriptionSourceState::Cancelled,
                source = next => source,
            }
        };
        match source {
            SubscriptionSourceState::Complete => {
                return Ok(
                    self.finish_monitored_source_output(session, SubscriptionOutput::Complete)
                );
            }
            SubscriptionSourceState::Cancelled => {
                return Ok(
                    self.finish_monitored_source_output(session, SubscriptionOutput::Cancelled)
                );
            }
            SubscriptionSourceState::Terminal(response) => {
                return Ok(self.finish_monitored_source_output(
                    session,
                    SubscriptionOutput::Terminal(response),
                ));
            }
            SubscriptionSourceState::EventReady => {}
        }
        // Source polling and callbacks are outside the admission lock. This
        // small transition decides whether fresh event work preceded shutdown.
        if !activity.start() {
            return Ok(self.finish_monitored_source_output(session, SubscriptionOutput::Cancelled));
        }
        let Some(sequence) = self
            .sequence
            .checked_add(1)
            .filter(|value| *value <= MAX_EVENT_SEQUENCE)
        else {
            return Ok(self.finish_monitored_source_output(
                session,
                terminal(
                    "Subscription event sequence exhausted",
                    "SUBSCRIPTION_LIMIT_EXCEEDED",
                ),
            ));
        };
        let captured = self.event_capture();
        let handoff = captured
            .as_ref()
            .map(|captured| captured.handoff_scope(InstrumentationKind::SubscriptionEvent));
        let payload_scope = handoff.clone();
        let work = async move {
            session.bind_event_instrumentation(payload_scope);
            let event_deadline = armer.arm(
                DeadlineRole::Event,
                self.state.subscriptions.limits().event_timeout,
            )?;
            let context_deadline =
                armer.arm(DeadlineRole::Phase, self.state.settings.context_timeout)?;
            let telemetry = self.state.schema.request_telemetry_with_capture(
                self.request.trace_id().cloned(),
                captured.as_ref(),
                InstrumentationKind::SubscriptionEvent,
            );
            let scope = telemetry.instrumentation_scope().cloned();
            session.bind_event_instrumentation(scope.clone());
            let mut outcome = LifecycleResult::Failed;
            let work = self.execute_monitored_event(
                session,
                sequence,
                armer,
                event_deadline,
                context_deadline,
                &telemetry,
                &mut outcome,
            );
            let result = match &scope {
                Some(scope) if activity.is_managed() => {
                    scope
                        .instrument(guard_instrumented_work(work, activity))
                        .await
                }
                Some(scope) => scope.instrument(work).await,
                None => work.await,
            };
            if let Some(scope) = scope {
                scope.finish(outcome);
            }
            result
        };
        match handoff {
            Some(scope) => scope.instrument(work).await,
            None => work.await,
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_monitored_event(
        &mut self,
        mut session: TaskOwnedValue<SubscriptionSession<C>>,
        sequence: u64,
        armer: &DeadlineArmer,
        mut event_deadline: crate::subscription::worker::MonitoredDeadline,
        mut context_deadline: crate::subscription::worker::MonitoredDeadline,
        telemetry: &RequestTelemetry,
        outcome: &mut LifecycleResult,
    ) -> Result<SubscriptionOutput, SubscriptionWorkerRuntimeError> {
        let fresh = prepare_context_monitored(
            &self.state.schema,
            &self.state.context_factory,
            &self.request,
            RequestPhase::SubscriptionEvent {
                transport: self.request.transport(),
                sequence,
            },
            armer.factory(),
            &mut context_deadline,
            &mut event_deadline,
            Some(telemetry),
        )
        .await?;
        if !matches!(fresh, ContextPreparation::TimedOut { .. }) {
            context_deadline.clear()?;
        }
        let fresh = match fresh {
            ContextPreparation::Ready(fresh) => fresh,
            ContextPreparation::Rejected { rejection, .. } => {
                *outcome = LifecycleResult::Rejected;
                event_deadline.clear()?;
                return Ok(self.finish_monitored_source_output(
                    session,
                    SubscriptionOutput::Terminal(SubscriptionTerminalResponse::execution(
                        GraphQlResponse::request_error(rejection.error().clone()),
                    )),
                ));
            }
            ContextPreparation::Internal { error, telemetry } => {
                *outcome = LifecycleResult::Failed;
                let response = self.internal_failure(&error, &telemetry);
                event_deadline.clear()?;
                return Ok(self.finish_monitored_source_output(
                    session,
                    SubscriptionOutput::Terminal(SubscriptionTerminalResponse::execution(response)),
                ));
            }
            ContextPreparation::TimedOut { .. } => {
                *outcome = LifecycleResult::TimedOut;
                return Ok(self.finish_monitored_source_output(
                    session,
                    SubscriptionOutput::Terminal(SubscriptionTerminalResponse::execution(
                        crate::timeout::request_timeout_response(),
                    )),
                ));
            }
        };
        let mut execution_deadline =
            armer.arm(DeadlineRole::Phase, self.state.settings.execution_timeout)?;
        let result = run_monitored(
            &mut execution_deadline,
            &mut event_deadline,
            session.execute_event(
                fresh.execution_context(),
                Some(fresh.telemetry()),
                Some(armer.factory()),
            ),
        )
        .await;
        if result.is_ok() {
            execution_deadline.clear()?;
            event_deadline.clear()?;
        }
        drop(fresh);
        Ok(match result {
            Ok(SubscriptionEventResult::Event(response)) => {
                *outcome = super::response_result(&response);
                self.sequence = sequence;
                self.session = Some(session.into_inner());
                SubscriptionOutput::Event(response)
            }
            Ok(SubscriptionEventResult::Terminal(response)) => {
                *outcome = LifecycleResult::Failed;
                self.finish_monitored_source_output(session, SubscriptionOutput::Terminal(response))
            }
            Ok(SubscriptionEventResult::Cancelled) => {
                *outcome = LifecycleResult::Cancelled;
                self.finish_monitored_source_output(session, SubscriptionOutput::Cancelled)
            }
            Err(_) => {
                *outcome = LifecycleResult::TimedOut;
                self.finish_monitored_source_output(
                    session,
                    SubscriptionOutput::Terminal(SubscriptionTerminalResponse::execution(
                        crate::timeout::request_timeout_response(),
                    )),
                )
            }
        })
    }

    fn finish_monitored_source_output(
        &mut self,
        session: TaskOwnedValue<SubscriptionSession<C>>,
        output: SubscriptionOutput,
    ) -> SubscriptionOutput {
        if session.host_observation_receipt().is_some() {
            self.session = Some(session.into_inner());
        }
        output
    }
}
