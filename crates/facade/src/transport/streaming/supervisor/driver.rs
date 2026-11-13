//! One application-polled aggregate, with exactly one live worker poll owner.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use super::HttpStreamingError;
use super::clock::StreamClock;
use super::entry::Entry;
use super::registry::Registry;

#[cfg(test)]
mod test_faults;

/// Body-independent supervision owned and polled by the application.
///
/// Poll this beside the server or spawn one explicitly tracked application
/// task and join it. Chigiri never spawns this driver. Stop routes/registration,
/// await this drain, then drain the endpoint runtime guard. A synchronously
/// blocking application poll requires another executor thread for supervision.
#[must_use = "HTTP streaming requires application-driven supervision and drain"]
pub struct HttpStreamingDriver {
    registry: Arc<Registry>,
    clock: Arc<StreamClock>,
    entries: Vec<Option<Entry>>,
    cursor: usize,
    timer: Option<(u64, Pin<Box<tokio::time::Sleep>>)>,
    result: Option<Result<(), HttpStreamingError>>,
    graceful: bool,
}

impl HttpStreamingDriver {
    pub(crate) fn graceful(&mut self) {
        self.graceful = true;
    }

    pub(crate) fn fail(&mut self, error: HttpStreamingError) {
        self.result = Some(Err(error));
        self.abandon(Some(error));
    }

    pub(super) fn new(
        registry: Arc<Registry>,
        clock: Arc<StreamClock>,
        maximum: usize,
    ) -> Result<Self, HttpStreamingError> {
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(maximum)
            .map_err(|_| HttpStreamingError::Capacity)?;
        entries.resize_with(maximum, || None);
        Ok(Self {
            registry,
            clock,
            entries,
            cursor: 0,
            timer: None,
            result: None,
            graceful: false,
        })
    }

    fn poll_entries(&mut self, context: &mut Context<'_>) -> Result<Poll<()>, HttpStreamingError> {
        let replacement = context.waker().clone();
        let shutdown = {
            let mut state = match self.registry.state.lock() {
                Ok(state) => state,
                Err(poisoned) => {
                    drop(poisoned);
                    drop(replacement);
                    return Err(HttpStreamingError::Invariant);
                }
            };
            let previous = state.driver.replace(replacement);
            if state
                .slots
                .iter()
                .enumerate()
                .any(|(index, slot)| slot.pending.is_some() && self.entries[index].is_some())
            {
                drop(state);
                drop(previous);
                return Err(HttpStreamingError::Invariant);
            }
            for (index, slot) in state.slots.iter_mut().enumerate() {
                if let Some(entry) = slot.pending.take() {
                    self.entries[index] = Some(entry);
                }
            }
            let shutdown = !state.open;
            drop(state);
            drop(previous);
            shutdown
        };
        let mut deadline = None::<u64>;
        let mut any = false;
        for offset in 0..self.entries.len() {
            let index = (self.cursor + offset) % self.entries.len();
            let Some(entry) = &mut self.entries[index] else {
                continue;
            };
            any = true;
            if shutdown && self.graceful {
                entry.begin_graceful();
            }
            let complete = match entry.poll(context, shutdown && !self.graceful) {
                Ok(complete) => complete,
                Err(error) => {
                    entry.fail(error);
                    context.waker().wake_by_ref();
                    false
                }
            };
            if complete {
                drop(self.entries[index].take());
                let mut state = self
                    .registry
                    .state
                    .lock()
                    .map_err(|_| HttpStreamingError::Invariant)?;
                state.slots[index].occupied = false;
            } else {
                match entry.next_deadline() {
                    Ok(Some(next)) => deadline = Some(deadline.map_or(next, |old| old.min(next))),
                    Ok(None) => {}
                    Err(error) => {
                        entry.fail(error);
                        context.waker().wake_by_ref();
                    }
                }
            }
        }
        self.cursor = (self.cursor + 1) % self.entries.len();
        if shutdown && (!any || self.entries.iter().all(Option::is_none)) {
            return Ok(Poll::Ready(()));
        }
        match deadline {
            Some(next) => {
                if self.timer.as_ref().is_none_or(|(old, _)| *old != next) {
                    self.timer = Some((
                        next,
                        Box::pin(tokio::time::sleep_until(self.clock.deadline(next)?)),
                    ));
                }
                if self
                    .timer
                    .as_mut()
                    .expect("armed aggregate deadline")
                    .1
                    .as_mut()
                    .poll(context)
                    .is_ready()
                {
                    self.timer.take();
                    context.waker().wake_by_ref();
                }
            }
            None => {
                self.timer.take();
            }
        }
        Ok(Poll::Pending)
    }

    fn abandon(&mut self, error: Option<HttpStreamingError>) {
        use chigiri_runtime::__private::SubscriptionTermination;
        let (slots, wake, decision) = {
            let mut state = self
                .registry
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            // Observe an application's earlier registration close before this
            // abandonment closes admission itself. The independent monotonic
            // authority survives poison without trusting mailbox contents.
            let decision = if self.registry.shutdown_requested() {
                SubscriptionTermination::ServerShutdown
            } else {
                SubscriptionTermination::Internal
            };
            state.open = false;
            let wake = state.driver.take();
            (std::mem::take(&mut state.slots), wake, decision)
        };
        if let Some(wake) = wake {
            wake.wake();
        }
        for mut entry in self.entries.iter_mut().filter_map(Option::take) {
            entry.cancel_with(decision, error);
        }
        for mut entry in slots.into_iter().filter_map(|slot| slot.pending) {
            entry.cancel_with(decision, error);
        }
    }
}

impl Future for HttpStreamingDriver {
    type Output = Result<(), HttpStreamingError>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(result) = self.result {
            return Poll::Ready(result);
        }
        match self.poll_entries(context) {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(())) => {
                self.result = Some(Ok(()));
                Poll::Ready(Ok(()))
            }
            Err(error) => {
                self.result = Some(Err(error));
                self.abandon(Some(error));
                Poll::Ready(Err(error))
            }
        }
    }
}

impl Drop for HttpStreamingDriver {
    fn drop(&mut self) {
        self.abandon(None);
    }
}
