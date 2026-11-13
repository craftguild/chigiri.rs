//! One validation result stays task-owned until the SDK accepts its transfer.

#[cfg(test)]
mod tests;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use tokio::sync::oneshot;

use crate::managed::{EventActivity, HttpFaultReporter};
use crate::subscription::worker::{SubscriptionWorkerRuntimeError, dispose_payload, dispose_value};

struct State<T> {
    value: Option<T>,
    accepted: bool,
}

pub(super) fn channel<T>(
    activity: EventActivity,
    reporter: Option<HttpFaultReporter>,
) -> (Sender<T>, Receiver<T>) {
    let shared = Arc::new(Mutex::new(State {
        value: None,
        accepted: false,
    }));
    let (ready, readiness) = oneshot::channel();
    let (acknowledge, acknowledged) = oneshot::channel();
    (
        Sender {
            shared: shared.clone(),
            ready: Some(ready),
            acknowledged,
            activity: activity.clone(),
        },
        Receiver {
            shared,
            readiness,
            acknowledge: Some(acknowledge),
            activity,
            reporter,
        },
    )
}

pub(super) struct Sender<T> {
    shared: Arc<Mutex<State<T>>>,
    ready: Option<oneshot::Sender<()>>,
    acknowledged: oneshot::Receiver<()>,
    activity: EventActivity,
}

impl<T> Sender<T> {
    pub(super) async fn offer(mut self, value: T) -> Result<(), SubscriptionWorkerRuntimeError> {
        {
            let mut state = self
                .shared
                .lock()
                .map_err(|_| SubscriptionWorkerRuntimeError::Invariant)?;
            state.value = Some(value);
        }
        let _ = self.ready.take().expect("one validation offer").send(());
        tokio::select! {
            biased;
            _ = self.activity.source_stopped() => {
                if self.activity.control().is_some_and(|control| control.outcome() == Some(crate::ShutdownOutcome::InfrastructureFailure)) {
                    Err(SubscriptionWorkerRuntimeError::Invariant)
                } else {
                    Err(SubscriptionWorkerRuntimeError::EndpointClosed)
                }
            }
            _ = &mut self.acknowledged => Ok(()),
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        let value = {
            let mut state = self
                .shared
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.accepted {
                None
            } else {
                state.value.take()
            }
        };
        if !value.is_none_or(dispose_value) {
            self.activity.poison_task();
        }
    }
}

pub(in crate::subscription::worker) struct Receiver<T> {
    shared: Arc<Mutex<State<T>>>,
    readiness: oneshot::Receiver<()>,
    acknowledge: Option<oneshot::Sender<()>>,
    activity: EventActivity,
    reporter: Option<HttpFaultReporter>,
}

impl<T> Receiver<T> {
    pub(super) fn poll_accept(&mut self, context: &mut Context<'_>) -> Poll<Result<(), ()>> {
        use std::future::Future;
        use std::pin::Pin;
        if self.acknowledge.is_none() {
            return Poll::Ready(Ok(()));
        }
        let Poll::Ready(ready) = Pin::new(&mut self.readiness).poll(context) else {
            return Poll::Pending;
        };
        if ready.is_err() {
            self.acknowledge.take();
            return Poll::Ready(Err(()));
        }
        let accepted = match self.shared.lock() {
            Ok(mut state) => {
                state.accepted = state.value.is_some();
                state.accepted
            }
            Err(_) => false,
        };
        let acknowledge = self.acknowledge.take().expect("one validation acceptance");
        if accepted {
            let _ = acknowledge.send(());
            Poll::Ready(Ok(()))
        } else {
            // Sender destruction may have reclaimed an unaccepted result
            // after readiness. Never poll that consumed readiness again.
            drop(acknowledge);
            Poll::Ready(Err(()))
        }
    }

    pub(super) fn take(&mut self) -> Option<T> {
        self.shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .value
            .take()
    }

    pub(in crate::subscription::worker) fn activity(&self) -> &EventActivity {
        &self.activity
    }

    /// Unaccepted values remain in the concrete task. Accepted values belong to
    /// this SDK owner and are destroyed before its original worker can retire.
    pub(in crate::subscription::worker) fn close(&mut self) {
        let (value, accepted) = {
            let mut state = self
                .shared
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.accepted {
                (state.value.take(), true)
            } else {
                (None, false)
            }
        };
        if !value.is_none_or(dispose_value) {
            self.activity.poison_task();
        }
        if accepted {
            self.finish_observation();
        } else if !self.reporter.take().is_none_or(dispose_value) {
            // Until acceptance, the original guarded task still owns private
            // failure observation, including blocked payload/future disposal.
            self.activity.poison_task();
        }
        self.readiness.close();
        self.acknowledge.take();
    }

    /// After transfer, this SDK task owns rejected output and the observer.
    /// Finish both before native join can release the original worker slot.
    pub(in crate::subscription::worker) fn finish_observation(&mut self) {
        if self.activity.task_failed()
            && let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
                if let Some(reporter) = &self.reporter {
                    reporter.report();
                }
            }))
            && !dispose_payload(payload)
        {
            self.activity.poison_task();
        }
        if !self.reporter.take().is_none_or(dispose_value) {
            self.activity.poison_task();
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.close();
    }
}
