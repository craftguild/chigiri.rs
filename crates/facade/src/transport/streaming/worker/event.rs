//! Closed event-demand forwarding without exposing the concrete task handle.

use std::task::{Context, Poll};

use crate::subscription::driver::SubscriptionOutput;
use crate::subscription::outbox::SubscriptionEventReservation;
use crate::subscription::worker::{ActiveSubscriptionWorker, ActiveWorkerSignal};

/// Sole event worker owner, issued only after bounded initial response commit.
#[doc(hidden)]
pub struct HttpSubscriptionEvents {
    worker: ActiveSubscriptionWorker,
    cancelling: bool,
}

impl HttpSubscriptionEvents {
    pub(crate) fn activity(&self) -> &crate::managed::EventActivity {
        &self.worker.activity
    }

    pub(crate) fn observe_drops(
        &mut self,
        outbox: &mut crate::subscription::outbox::SubscriptionOutbox,
    ) {
        self.worker.observe_drops(outbox);
    }

    pub(crate) fn finish_observation(
        &mut self,
        decision: chigiri_runtime::__private::SubscriptionTermination,
        total: u64,
    ) {
        if self.worker.finish_observation(decision, total) {
            self.cancel();
        }
    }

    pub(crate) fn logical_terminal_selected(&self) -> bool {
        self.worker.logical_terminal_selected()
    }

    pub(super) const fn new(worker: ActiveSubscriptionWorker) -> Self {
        Self {
            worker,
            cancelling: false,
        }
    }

    /// Revokes this generation and starts its existing bounded local cleanup.
    ///
    /// Continue polling until Cancelled, including when no body is polled.
    /// Dropping this owner instead transfers unfinished work to quarantine.
    pub fn cancel(&mut self) {
        if !self.cancelling {
            self.cancelling = true;
            self.worker.cancel();
        }
    }

    /// Transfers one already acquired maximum-event reservation before demand.
    pub fn request_next(
        &mut self,
        reservation: SubscriptionEventReservation,
    ) -> Result<(), SubscriptionEventReservation> {
        if self.cancelling {
            return Err(reservation);
        }
        self.worker.request_event(reservation)
    }

    pub(crate) fn request_next_deferred(
        &mut self,
        reservation: SubscriptionEventReservation,
    ) -> Result<Option<std::task::Waker>, SubscriptionEventReservation> {
        if self.cancelling {
            return Err(reservation);
        }
        self.worker.request_event_deferred(reservation)
    }

    /// Drives worker deadlines and receives at most one sequential event result.
    ///
    /// Official supervision must call this even when the transport body is not
    /// polled. A body poller alone cannot enforce blocked application deadlines.
    pub fn poll_worker(&mut self, context: &mut Context<'_>) -> Poll<HttpSubscriptionSignal> {
        self.worker.poll_signal(context).map(|signal| {
            if self.cancelling {
                return HttpSubscriptionSignal::Cancelled;
            }
            match signal {
                ActiveWorkerSignal::Cancelled => HttpSubscriptionSignal::Cancelled,
                ActiveWorkerSignal::Event(event) => {
                    let (reservation, output) = event.into_parts();
                    HttpSubscriptionSignal::Event {
                        reservation,
                        output,
                    }
                }
                ActiveWorkerSignal::Finished => HttpSubscriptionSignal::Finished,
                ActiveWorkerSignal::TimedOut => HttpSubscriptionSignal::TimedOut,
                ActiveWorkerSignal::Failed => HttpSubscriptionSignal::Failed,
            }
        })
    }
}

/// One closed observation from the concrete sequential event worker.
#[doc(hidden)]
pub enum HttpSubscriptionSignal {
    /// Explicit cancellation completed its bounded wait or transferred to quarantine.
    /// No event or terminal wire message may be emitted for this generation.
    Cancelled,
    /// A result reunited with the exact reservation acquired before its demand.
    Event {
        /// The one outer-owned maximum-event reservation.
        reservation: SubscriptionEventReservation,
        /// The ordinary result or safe terminal output from neutral execution.
        output: SubscriptionOutput,
    },
    /// The source task and its returned values have physically completed.
    Finished,
    /// A worker deadline terminated this generation after bounded cleanup.
    TimedOut,
    /// The worker failed; this is not natural source completion.
    Failed,
}
