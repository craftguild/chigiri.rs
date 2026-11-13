//! Outside acceptance of the one bounded setup decision authority.

use std::pin::Pin;
use std::task::{Context, Poll};

use super::{SubscriptionTerminalCause, WebSocketSetupMessage, WebSocketSubscriptionSetup};
use crate::subscription::worker::SubscriptionWorkerRuntimeError;
use crate::subscription::worker::port::PortSender;
use crate::subscription::worker::websocket::WebSocketSubscriptionSetupOutcome;

impl Future for WebSocketSubscriptionSetup {
    type Output = WebSocketSubscriptionSetupOutcome;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let subscription = self
            .subscription
            .as_mut()
            .expect("subscription setup worker");
        match subscription.poll_deadline(context, None) {
            Poll::Ready(Ok(terminal)) => {
                self.subscription.take();
                return Poll::Ready(WebSocketSubscriptionSetupOutcome::Terminal(terminal));
            }
            Poll::Ready(Err(error)) => {
                self.subscription.take();
                return Poll::Ready(WebSocketSubscriptionSetupOutcome::SharedFailure(error));
            }
            Poll::Pending => {}
        }
        match self.setup_receiver.poll_take(context) {
            Poll::Ready(Ok(Some(WebSocketSetupMessage::Observation(mut observation)))) => {
                let subscription = self
                    .subscription
                    .as_mut()
                    .expect("subscription setup worker");
                if subscription.observation.is_some() || !observation.accept() {
                    return self.finish_failed(SubscriptionWorkerRuntimeError::Invariant);
                }
                observation.poll(context);
                subscription.observation = Some(observation);
                context.waker().wake_by_ref();
                Poll::Pending
            }
            Poll::Ready(Ok(Some(WebSocketSetupMessage::Ready {
                overflow,
                receipt,
                telemetry,
            }))) => {
                let subscription = self
                    .subscription
                    .as_mut()
                    .expect("subscription setup worker");
                let valid = match (&subscription.observation, receipt.as_ref()) {
                    (Some(owner), receipt) => owner.registered(receipt, overflow),
                    (None, None) => true,
                    _ => false,
                };
                if !valid {
                    return self.finish_failed(SubscriptionWorkerRuntimeError::Invariant);
                }
                if let Some(failure) = &mut subscription.failure {
                    failure.bind_retained_telemetry(telemetry.as_ref());
                }
                Poll::Ready(WebSocketSubscriptionSetupOutcome::Ready {
                    subscription: Box::new(
                        self.subscription.take().expect("subscription setup worker"),
                    ),
                    overflow,
                })
            }
            Poll::Ready(Ok(Some(WebSocketSetupMessage::Response(response)))) => {
                let terminal = self
                    .subscription
                    .as_mut()
                    .expect("subscription setup worker")
                    .terminal_response(response);
                self.subscription.take();
                Poll::Ready(match terminal {
                    Ok(terminal) => WebSocketSubscriptionSetupOutcome::Terminal(terminal),
                    Err(error) => WebSocketSubscriptionSetupOutcome::SharedFailure(error),
                })
            }
            Poll::Ready(Ok(None)) => {
                if self
                    .subscription
                    .as_ref()
                    .expect("subscription setup worker")
                    .activity
                    .setup_cancelled()
                {
                    let terminal = self
                        .subscription
                        .as_mut()
                        .expect("subscription setup worker")
                        .begin_terminal(
                            super::super::WebSocketTerminalMessage::Cancelled,
                            chigiri_runtime::__private::SubscriptionTermination::ServerShutdown,
                            None,
                        );
                    self.subscription.take();
                    Poll::Ready(match terminal {
                        Ok(terminal) => WebSocketSubscriptionSetupOutcome::Terminal(terminal),
                        Err(error) => WebSocketSubscriptionSetupOutcome::SharedFailure(error),
                    })
                } else {
                    self.finish_failed(SubscriptionWorkerRuntimeError::Shutdown)
                }
            }
            Poll::Ready(Err(error)) => self.finish_failed(error),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl crate::subscription::worker::observation::HostObservationHandoff
    for PortSender<WebSocketSetupMessage>
{
    fn transfer(
        &self,
        observation: chigiri_runtime::__private::SubscriptionObservation,
    ) -> Result<(), chigiri_runtime::__private::SubscriptionObservation> {
        self.try_send(WebSocketSetupMessage::Observation(observation))
            .map_err(|message| {
                let WebSocketSetupMessage::Observation(observation) = message else {
                    unreachable!()
                };
                observation
            })
    }
}

impl WebSocketSubscriptionSetup {
    #[cfg(test)]
    pub(crate) fn is_finished_for_test(&self) -> bool {
        self.subscription
            .as_ref()
            .and_then(|subscription| subscription.worker.as_ref())
            .is_some_and(crate::subscription::worker::ConcreteWorker::is_finished_for_test)
    }

    #[cfg(test)]
    pub(crate) fn observe_setup_close_for_test(
        &mut self,
        observe: impl Fn(bool) + Send + Sync + 'static,
    ) {
        struct Observer<F> {
            activity: crate::managed::EventActivity,
            observe: F,
        }
        impl<F: Fn(bool) + Send + Sync + 'static> std::task::Wake for Observer<F> {
            fn wake(self: std::sync::Arc<Self>) {
                (self.observe)(self.activity.task_failed());
            }
        }
        let activity = self.subscription.as_ref().unwrap().activity.clone();
        let waker = std::task::Waker::from(std::sync::Arc::new(Observer { activity, observe }));
        assert!(
            self.setup_receiver
                .poll_closed(&mut Context::from_waker(&waker))
                .is_pending()
        );
    }

    fn finish_failed(
        &mut self,
        error: SubscriptionWorkerRuntimeError,
    ) -> Poll<WebSocketSubscriptionSetupOutcome> {
        let terminal = self
            .subscription
            .as_mut()
            .expect("subscription setup worker")
            .terminal_cause(SubscriptionTerminalCause::Failed(error));
        self.subscription.take();
        Poll::Ready(match terminal {
            Ok(terminal) => WebSocketSubscriptionSetupOutcome::Terminal(terminal),
            Err(error) => WebSocketSubscriptionSetupOutcome::SharedFailure(error),
        })
    }
}
