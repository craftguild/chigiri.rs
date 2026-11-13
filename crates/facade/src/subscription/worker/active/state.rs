//! Terminal transitions that retain task-adjacent state through physical join.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use chigiri_http::ResponseCompletionOutcome;

use super::{
    ActiveSubscriptionWorker, ActiveWorkerSignal, SubscriptionWorkerEvent, TerminalReason,
};
use crate::subscription::bridge_resources::{BridgeCleanupOutcome, DetachedCleanup};
use crate::subscription::worker::{
    ConcreteWorkerOutcome, MonitoredDeadlineTable, RetiredWorkerOwner, WorkerRetainedState,
};

impl ActiveSubscriptionWorker {
    pub(super) fn poll_terminal(&mut self, context: &mut Context<'_>) -> Poll<ActiveWorkerSignal> {
        if let Some(observation) = &mut self.observation {
            observation.poll(context);
        }
        if let Some(cleanup) = &mut self.cleanup {
            let outcome = match Pin::new(cleanup).poll(context) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(outcome) => outcome,
            };
            self.cleanup.take();
            if outcome == BridgeCleanupOutcome::Completed
                && self.initial_response.terminal_failed(context)
            {
                self.terminal = Some(TerminalReason::Failed);
            }
            return Poll::Ready(
                match self.terminal.take().unwrap_or(TerminalReason::Failed) {
                    TerminalReason::TimedOut => ActiveWorkerSignal::TimedOut,
                    TerminalReason::Failed => ActiveWorkerSignal::Failed,
                    TerminalReason::Cancelled(_) => ActiveWorkerSignal::Cancelled,
                },
            );
        }
        if let Some(deadlines) = &mut self.deadlines {
            let reason = deadline_reason(deadlines.poll_expired(context));
            if let Some(reason) = reason {
                self.begin_cleanup(reason);
                return self.poll_terminal(context);
            }
        }
        Poll::Pending
    }

    pub(super) fn poll_join(&mut self, context: &mut Context<'_>) -> Poll<Result<(), ()>> {
        self.poll_join_output(context).map(|result| match result {
            Ok(None) => Ok(()),
            Ok(Some(_)) | Err(()) => Err(()),
        })
    }

    pub(super) fn poll_join_output(
        &mut self,
        context: &mut Context<'_>,
    ) -> Poll<Result<Option<SubscriptionWorkerEvent>, ()>> {
        if self.ended {
            return Poll::Ready(Ok(None));
        }
        let Some(worker) = &mut self.worker else {
            return Poll::Ready(Err(()));
        };
        let outcome = match Pin::new(worker).poll(context) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(outcome) => outcome,
        };
        match outcome {
            ConcreteWorkerOutcome::Completed(Ok(())) => {
                self.worker.take();
                Poll::Ready(self.finish_joined())
            }
            ConcreteWorkerOutcome::Cancelled
            | ConcreteWorkerOutcome::Panicked
            | ConcreteWorkerOutcome::Completed(Err(_)) => {
                self.worker.take();
                drop(self.finish_joined());
                Poll::Ready(Err(()))
            }
            ConcreteWorkerOutcome::Poisoned => {
                self.begin_cleanup(TerminalReason::Failed);
                Poll::Pending
            }
        }
    }

    pub(super) fn begin_cleanup(&mut self, reason: TerminalReason) {
        if let Some(observation) = &mut self.observation {
            observation.finish_current(match reason {
                TerminalReason::TimedOut => {
                    chigiri_runtime::__private::SubscriptionTermination::ExecutionFailed
                }
                TerminalReason::Failed => {
                    chigiri_runtime::__private::SubscriptionTermination::Internal
                }
                TerminalReason::Cancelled(_) => {
                    chigiri_runtime::__private::SubscriptionTermination::ServerShutdown
                }
            });
        }
        if self.cleanup.is_some() || self.worker.is_none() {
            self.terminal.get_or_insert(reason);
            return;
        }
        self.demand_sender.close();
        self.event_receiver.close();
        let initial_was_awaiting = self.initial_response.is_awaiting();
        match reason {
            TerminalReason::TimedOut => self
                .initial_response
                .terminate(ResponseCompletionOutcome::TimedOut),
            TerminalReason::Failed => self.initial_response.abandon(),
            TerminalReason::Cancelled(outcome) => self.initial_response.terminate(outcome),
        }
        let Some(deadlines) = self.deadlines.take() else {
            return self.fail_closed();
        };
        if matches!(reason, TerminalReason::TimedOut) || !initial_was_awaiting {
            deadlines.close();
        }
        let Some(operation) = self.operation.take() else {
            return self.fail_closed_with_deadlines(deadlines);
        };
        let terminal = match operation.revoke() {
            Ok(terminal) => terminal,
            Err(_) => return self.fail_closed_with_deadlines(deadlines),
        };
        let worker = self.worker.take().expect("active concrete worker");
        let retained = WorkerRetainedState::active(
            deadlines,
            self.pending_reservation.take(),
            self.initial_resources.take(),
        );
        let cleanup = DetachedCleanup::worker(worker.erase(), retained);
        self.cleanup = Some(terminal.coordinate(cleanup, self.deadline_factory));
        self.terminal = Some(reason);
    }

    fn finish_joined(&mut self) -> Result<Option<SubscriptionWorkerEvent>, ()> {
        self.demand_sender.close();
        // The worker may publish its final result between an empty receive
        // and JoinReady. Close and transfer that result with its existing grant.
        let output = self.event_receiver.close_and_take();
        let reservation = self.pending_reservation.take();
        self.initial_response.abandon();
        self.operation.take();
        if let Some(initial) = &mut self.initial_resources {
            initial.release_fallbacks();
        }
        self.ended = true;
        match (output, reservation) {
            (Ok(Some(output)), Some(reservation)) => {
                Ok(Some(SubscriptionWorkerEvent::new(reservation, output)))
            }
            (Ok(None), _) => Ok(None),
            (Err(_), _) | (Ok(Some(_)), None) => Err(()),
        }
    }

    fn fail_closed(&mut self) {
        let retained = WorkerRetainedState::orphan_active(
            self.deadlines.take(),
            self.pending_reservation.take(),
            self.initial_resources.take(),
        );
        if let Some(worker) = self.worker.take() {
            worker.abort();
            worker.erase().retire(RetiredWorkerOwner::orphan(retained));
        } else {
            std::mem::forget(retained);
        }
        if let Some(operation) = self.operation.take() {
            std::mem::forget(operation);
        }
        self.terminal = Some(TerminalReason::Failed);
    }

    fn fail_closed_with_deadlines(&mut self, deadlines: MonitoredDeadlineTable) {
        self.deadlines = Some(deadlines);
        self.fail_closed();
    }

    pub(super) fn managed_stop_reason(&self) -> Option<TerminalReason> {
        let control = self.activity.control()?;
        if !control.is_stopping()
            || control.outcome() == Some(crate::ShutdownOutcome::InfrastructureFailure)
        {
            return None;
        }
        Some(TerminalReason::Cancelled(
            if control.outcome() == Some(crate::ShutdownOutcome::Expired) {
                ResponseCompletionOutcome::TimedOut
            } else {
                ResponseCompletionOutcome::Failed
            },
        ))
    }
}

fn deadline_reason<T>(
    result: Poll<Result<T, crate::subscription::worker::SubscriptionWorkerRuntimeError>>,
) -> Option<TerminalReason> {
    match result {
        Poll::Ready(Ok(_)) => Some(TerminalReason::TimedOut),
        Poll::Ready(Err(_)) => Some(TerminalReason::Failed),
        Poll::Pending => None,
    }
}

impl Drop for ActiveSubscriptionWorker {
    fn drop(&mut self) {
        if self.worker.is_some() {
            self.begin_cleanup(self.managed_stop_reason().unwrap_or(TerminalReason::Failed));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_deadline_table_is_failure_not_elapsed_time() {
        let mut table =
            MonitoredDeadlineTable::with_overall(Box::pin(std::future::pending())).unwrap();
        let mut context = Context::from_waker(std::task::Waker::noop());
        assert!(deadline_reason(table.poll_expired(&mut context)).is_none());
        table.close();
        assert!(matches!(
            deadline_reason(table.poll_expired(&mut context)),
            Some(TerminalReason::Failed)
        ));
    }

    #[test]
    fn actual_elapsed_deadline_is_timeout_not_internal_failure() {
        let mut table =
            MonitoredDeadlineTable::with_overall(Box::pin(std::future::ready(()))).unwrap();
        let mut context = Context::from_waker(std::task::Waker::noop());
        assert!(matches!(
            deadline_reason(table.poll_expired(&mut context)),
            Some(TerminalReason::TimedOut)
        ));
    }

    struct SendBeforeJoin {
        release: Option<tokio::sync::oneshot::Sender<()>>,
        task: tokio::task::AbortHandle,
    }

    impl std::task::Wake for SendBeforeJoin {
        fn wake(self: std::sync::Arc<Self>) {}
    }

    impl Drop for SendBeforeJoin {
        fn drop(&mut self) {
            self.release.take().unwrap().send(()).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while !self.task.is_finished() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "owned worker did not join"
                );
                std::thread::yield_now();
            }
        }
    }

    struct Never;
    impl chigiri_http::DeadlineFactory for Never {
        fn sleep(&self, _: std::time::Duration) -> chigiri_http::BoxFuture<'_, ()> {
            Box::pin(std::future::pending())
        }
    }

    fn final_result_at_join(join_fails: bool) {
        use crate::subscription::bridge_resources::{BridgeOperationKind, BridgeResourceOwner};
        use crate::subscription::driver::SubscriptionOutput;
        use crate::subscription::outbox::SubscriptionOutbox;
        use crate::subscription::settings::{
            SubscriptionLimits, SubscriptionProtocolSettings, ValidatedSubscriptionSettings,
        };
        use crate::subscription::worker::{SubscriptionWorkerRuntimeError, port::channel};
        use std::sync::Arc;
        use std::task::Waker;
        use std::time::Duration;

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_time()
            .build()
            .unwrap();
        let settings = ValidatedSubscriptionSettings::new(
            SubscriptionLimits {
                max_active_subscriptions: 1,
                max_pending_operation_setups: 2,
                max_encoded_event_bytes: 1024,
                max_endpoint_encoded_bytes: 1024,
                ..SubscriptionLimits::default()
            },
            SubscriptionProtocolSettings::default(),
        );
        let owner = BridgeResourceOwner::new(&settings).unwrap();
        let guard = owner.register_worker_runtime().unwrap();
        let operation = owner
            .begin_http_subscription()
            .unwrap()
            .activate(BridgeOperationKind::Subscription)
            .unwrap();
        let reservation = SubscriptionOutbox::new(
            &settings,
            owner.delivery().byte_owner(),
            chigiri_runtime::SubscriptionOverflow::Backpressure,
            chigiri_http::SubscriptionTransport::ServerSentEvents,
            None,
        )
        .unwrap()
        .reserve_before_poll(0)
        .unwrap();
        let (demand_sender, _demand_receiver) = channel();
        let (sender, mut receiver) = channel();
        let (release, wait) = tokio::sync::oneshot::channel();
        let concrete = {
            let _entered = runtime.enter();
            operation
                .spawn_worker(async move {
                    wait.await
                        .map_err(|_| SubscriptionWorkerRuntimeError::Shutdown)?;
                    sender
                        .try_send(SubscriptionOutput::Complete)
                        .map_err(|_| SubscriptionWorkerRuntimeError::Shutdown)?;
                    if join_fails {
                        Err(SubscriptionWorkerRuntimeError::Invariant)
                    } else {
                        Ok(())
                    }
                })
                .unwrap()
        };
        let waker = Waker::from(Arc::new(SendBeforeJoin {
            release: Some(release),
            task: concrete.abort_handle_for_test(),
        }));
        assert!(
            receiver
                .poll_take(&mut Context::from_waker(&waker))
                .is_pending()
        );
        // Only the port retains this waker. Its replacement runs after the
        // empty read has returned Pending and unlocked, before join is polled.
        drop(waker);
        let (initial_response, _initial_task) =
            super::super::initial_command::initial_response_channel();
        let mut worker = ActiveSubscriptionWorker {
            activity: crate::managed::EventActivity::new(None),
            observation: None,
            operation: Some(operation),
            worker: Some(concrete),
            deadlines: None,
            deadline_factory: &Never,
            demand_sender,
            event_receiver: receiver,
            initial_response,
            initial_resources: None,
            pending_reservation: Some(reservation),
            cleanup: None,
            terminal: None,
            ended: false,
            retained_telemetry: None,
        };
        let mut context = Context::from_waker(Waker::noop());
        let result = worker.poll_signal(&mut context);
        if join_fails {
            assert!(matches!(result, Poll::Ready(ActiveWorkerSignal::Failed)));
        } else {
            let Poll::Ready(ActiveWorkerSignal::Event(event)) = result else {
                panic!("successful join discarded the final subscription result");
            };
            let (reservation, output) = event.into_parts();
            assert!(matches!(output, SubscriptionOutput::Complete));
            drop(reservation);
            assert!(matches!(
                worker.poll_signal(&mut context),
                Poll::Ready(ActiveWorkerSignal::Finished)
            ));
        }
        drop(worker);
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(2), guard.shutdown().unwrap())
                .await
                .unwrap()
                .unwrap();
        });
    }

    #[test]
    fn terminal_result_sent_after_empty_read_survives_successful_join() {
        final_result_at_join(false);
    }

    #[test]
    fn failed_join_does_not_deliver_buffered_success() {
        final_result_at_join(true);
    }
}
