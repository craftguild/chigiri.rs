//! Own one bounded typed FIFO independently of source demand and VM execution.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};
use std::task::{Context, Poll};
use std::time::Instant;

use crate::events::{SourceLimits, retention::Retention};
use crate::luau::ScriptError;

use super::super::{CapturedEvent, PublicationError};
use super::queue::Queue;

pub(in crate::publication) struct SourcePort {
    state: Mutex<Queue>,
    delivery: Arc<chigiri::__private::HostSourceDelivery>,
    usage: Arc<super::accounting::Usage>,
    activation: OnceLock<Instant>,
    wake: Option<Arc<super::super::wake::DeliveryWake>>,
}

pub(in crate::publication) enum Offer {
    Accepted,
    Failed(CapturedEvent),
}

#[derive(Clone, Copy)]
pub(in crate::publication) enum Termination {
    Lost,
    Pressure,
    Age,
    Counter,
    Shutdown,
    Invariant,
    Sequence,
}

impl SourcePort {
    #[cfg(test)]
    pub(in crate::publication) fn set_drop_count_for_test(&self, value: u64) {
        self.state.lock().unwrap().dropped = value;
    }

    #[cfg(test)]
    pub(in crate::publication) fn drop_count_for_test(&self) -> u64 {
        self.state.lock().unwrap().dropped
    }
    pub(in crate::publication) fn admit(
        &self,
        admission: &super::admission::Admission,
    ) -> Result<(), PublicationError> {
        self.state
            .lock()
            .map_err(|_| PublicationError::Invariant)?
            .usage
            .admit(admission)
    }
    #[cfg(test)]
    pub(in crate::publication) fn new(
        limits: SourceLimits,
        retention: Retention,
    ) -> Result<Arc<Self>, PublicationError> {
        Self::with_wake(limits, retention, None)
    }

    pub(in crate::publication) fn with_wake(
        limits: SourceLimits,
        retention: Retention,
        wake: Option<Arc<super::super::wake::DeliveryWake>>,
    ) -> Result<Arc<Self>, PublicationError> {
        let queue = Queue::new(limits, retention)?;
        Ok(Arc::new(Self {
            delivery: queue.delivery.clone(),
            usage: queue.usage.clone(),
            state: Mutex::new(queue),
            activation: OnceLock::new(),
            wake,
        }))
    }

    pub(in crate::publication) fn source_delivery(
        &self,
    ) -> Arc<chigiri::__private::HostSourceDelivery> {
        self.delivery.clone()
    }

    pub(in crate::publication) fn configure(
        &self,
        overflow: chigiri::SubscriptionOverflow,
    ) -> Result<(), PublicationError> {
        self.state
            .lock()
            .map_err(|_| PublicationError::Invariant)?
            .overflow = overflow;
        Ok(())
    }

    pub(in crate::publication) fn is_closed(&self) -> bool {
        self.usage.closed.load(Ordering::Acquire)
    }

    pub(in crate::publication) fn activate(&self) {
        self.activation.get_or_init(Instant::now);
        if let Some(wake) = &self.wake {
            wake.notify();
        }
    }

    pub(in crate::publication) fn activated_at(&self) -> Option<Instant> {
        self.activation.get().copied()
    }

    #[cfg(test)]
    pub(in crate::publication) fn activate_at(&self, now: Instant) {
        self.activation.set(now).unwrap();
    }

    pub(in crate::publication) fn finish(&self) -> Result<(), PublicationError> {
        let wake = {
            let mut queue = self.state.lock().map_err(|_| PublicationError::Invariant)?;
            queue.finished = true;
            queue.waker.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
        Ok(())
    }

    pub(in crate::publication) fn input_usage(&self) -> Arc<super::accounting::Usage> {
        self.usage.clone()
    }

    pub(in crate::publication) fn relieve_staging_pressure(&self) -> Result<(), PublicationError> {
        let (wake, cancel) = {
            let mut queue = self.state.lock().map_err(|_| PublicationError::Invariant)?;
            queue.relieve_staging_pressure();
            if queue.closed {
                self.usage.closed.store(true, Ordering::Release);
            }
            (
                queue.waker.take(),
                queue.closed.then(|| queue.usage.cancel.clone()),
            )
        };
        if let Some(cancel) = cancel {
            cancel.cancel();
        }
        if let Some(waker) = wake {
            waker.wake();
        }
        Ok(())
    }

    pub(super) fn poll(
        &self,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<CapturedEvent, ScriptError>>> {
        self.poll_at(context, Instant::now())
    }

    pub(in crate::publication) fn poll_at(
        &self,
        context: &mut Context<'_>,
        now: Instant,
    ) -> Poll<Option<Result<CapturedEvent, ScriptError>>> {
        let replacement = context.waker().clone();
        let (result, retired, cancel) = {
            let mut queue = match self.state.lock() {
                Ok(queue) => queue,
                Err(_) => return Poll::Ready(Some(Err(failure(Termination::Invariant)))),
            };
            queue.expire(now);
            if queue.closed {
                self.usage.closed.store(true, Ordering::Release);
            }
            let cancel = queue.closed.then(|| queue.usage.cancel.clone());
            let (result, retired) = if let Some(error) = queue.terminal.take() {
                (Poll::Ready(Some(Err(error))), queue.waker.take())
            } else if let Some(mut entry) = queue.entries.pop_front() {
                entry.event.set_dropped(
                    (queue.overflow == chigiri::SubscriptionOverflow::DropOldest)
                        .then_some(queue.dropped),
                );
                (Poll::Ready(Some(Ok(entry.event))), queue.waker.take())
            } else if queue.closed || queue.finished {
                (Poll::Ready(None), queue.waker.take())
            } else {
                (Poll::Pending, queue.waker.replace(replacement))
            };
            (result, retired, cancel)
        };
        drop(retired);
        if let Some(cancel) = cancel {
            cancel.cancel();
        }
        result
    }

    pub(in crate::publication) fn offer_at(&self, event: CapturedEvent, now: Instant) -> Offer {
        let (wake, cancel) = {
            let mut queue = match self.state.lock() {
                Ok(queue) => queue,
                Err(_) => return Offer::Failed(event),
            };
            queue.offer(event, now);
            if queue.closed {
                self.usage.closed.store(true, Ordering::Release);
            }
            (
                queue.waker.take(),
                queue.closed.then(|| queue.usage.cancel.clone()),
            )
        };
        if let Some(cancel) = cancel {
            cancel.cancel();
        }
        if let Some(waker) = wake {
            waker.wake();
        }
        Offer::Accepted
    }

    pub(in crate::publication) fn expire(&self, now: Instant) -> Result<(), PublicationError> {
        let (wake, cancel) = {
            let mut queue = self.state.lock().map_err(|_| PublicationError::Invariant)?;
            queue.expire(now);
            if queue.closed {
                self.usage.closed.store(true, Ordering::Release);
            }
            (
                queue.waker.take(),
                queue.closed.then(|| queue.usage.cancel.clone()),
            )
        };
        if let Some(cancel) = cancel {
            cancel.cancel();
        }
        if let Some(waker) = wake {
            waker.wake();
        }
        Ok(())
    }

    pub(in crate::publication) fn terminate(
        &self,
        reason: Termination,
    ) -> Result<(), PublicationError> {
        let (wake, cancel) = {
            let mut queue = self.state.lock().map_err(|_| PublicationError::Invariant)?;
            queue.terminate(reason);
            self.usage.closed.store(true, Ordering::Release);
            (
                queue.waker.take(),
                queue.closed.then(|| queue.usage.cancel.clone()),
            )
        };
        if let Some(cancel) = cancel {
            cancel.cancel();
        }
        if let Some(waker) = wake {
            waker.wake();
        }
        Ok(())
    }

    pub(in crate::publication) fn close(&self) {
        let (wake, cancel) = {
            let Ok(mut queue) = self.state.lock() else {
                return;
            };
            queue.closed = true;
            queue.finished = true;
            queue.terminal = None;
            queue.delivery.freeze();
            queue.entries.clear();
            self.usage.closed.store(true, Ordering::Release);
            (
                queue.waker.take(),
                queue.closed.then(|| queue.usage.cancel.clone()),
            )
        };
        if let Some(cancel) = cancel {
            cancel.cancel();
        }
        drop(wake);
        if let Some(wake) = &self.wake {
            wake.notify();
        }
    }
}

pub(super) fn failure(reason: Termination) -> ScriptError {
    let message = match reason {
        Termination::Lost => "publication delivery was lost",
        Termination::Pressure => "source retained capacity exceeded",
        Termination::Age => "source queue age exceeded",
        Termination::Counter => "source delivery counter exhausted",
        Termination::Shutdown => "publication source stopped",
        Termination::Invariant => "publication source invariant failed",
        Termination::Sequence => "sequence tick counter exhausted",
    };
    ScriptError::new("source", message)
}
