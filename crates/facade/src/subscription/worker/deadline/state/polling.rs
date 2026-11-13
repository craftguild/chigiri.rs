//! Exact timer polling and deadline evidence captured before releasing its state.

use super::*;

impl DeadlineState {
    pub(super) fn poll_role(
        &self,
        role: DeadlineRole,
        context: &mut Context<'_>,
    ) -> Poll<Result<FiredDeadline, SubscriptionWorkerRuntimeError>> {
        let (generation, mut timer) = {
            let mut inner = match self.lock() {
                Ok(inner) => inner,
                Err(error) => return Poll::Ready(Err(error)),
            };
            if inner.closed || inner.corrupted {
                return Poll::Ready(Err(SubscriptionWorkerRuntimeError::Invariant));
            }
            let slot = &mut inner.slots[role.index()];
            let TimerState::Armed(_) = slot.timer else {
                return Poll::Pending;
            };
            let TimerState::Armed(timer) =
                std::mem::replace(&mut slot.timer, TimerState::Polling { clear: false })
            else {
                unreachable!("matched armed deadline")
            };
            (slot.generation, timer)
        };
        let polled = timer.as_mut().poll(context);
        let mut inner = match self.lock() {
            Ok(inner) => inner,
            Err(_) => {
                std::mem::forget(timer);
                return Poll::Ready(Err(SubscriptionWorkerRuntimeError::Invariant));
            }
        };
        let closed = inner.closed;
        let slot = &mut inner.slots[role.index()];
        // A task can clear and immediately re-arm a phase while its old timer
        // is outside the lock. The old poll cannot alter the newer generation.
        if slot.generation != generation {
            drop(inner);
            drop(timer);
            return Poll::Pending;
        }
        let TimerState::Polling { clear } = slot.timer else {
            std::mem::forget(timer);
            inner.corrupted = true;
            return Poll::Ready(Err(SubscriptionWorkerRuntimeError::Invariant));
        };
        if closed || clear {
            slot.timer = TimerState::Empty;
            drop(inner);
            drop(timer);
            return Poll::Pending;
        }
        match polled {
            Poll::Pending => {
                slot.timer = TimerState::Armed(timer);
                let first_observation = !slot.observed;
                slot.observed = true;
                let proxy = if first_observation {
                    slot.proxy_waker.take()
                } else {
                    None
                };
                drop(inner);
                if let Some(waker) = proxy {
                    waker.wake();
                }
                Poll::Pending
            }
            Poll::Ready(()) => {
                slot.timer = TimerState::Fired;
                slot.observed = true;
                let proxy = slot.proxy_waker.take();
                let http_phase = if role == DeadlineRole::Phase {
                    slot.http_phase
                } else {
                    let phase = &inner.slots[DeadlineRole::Phase.index()];
                    if matches!(
                        phase.timer,
                        TimerState::Armed(_)
                            | TimerState::Polling { clear: false }
                            | TimerState::Fired
                    ) {
                        phase.http_phase
                    } else {
                        None
                    }
                };
                drop(inner);
                drop(timer);
                if let Some(waker) = proxy {
                    waker.wake();
                }
                Poll::Ready(Ok(FiredDeadline {
                    role,
                    generation,
                    http_phase,
                }))
            }
        }
    }
}
