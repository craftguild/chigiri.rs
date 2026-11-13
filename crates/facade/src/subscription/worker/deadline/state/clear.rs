//! Exact-generation deadline clearing with external disposal after unlocking.

use super::{DeadlineState, TimerState};
use crate::subscription::worker::deadline::DeadlineRole;
use crate::subscription::worker::error::SubscriptionWorkerRuntimeError;

impl DeadlineState {
    pub(in crate::subscription::worker::deadline) fn clear(
        &self,
        role: DeadlineRole,
        generation: u64,
    ) -> Result<(), SubscriptionWorkerRuntimeError> {
        let mut inner = self.lock()?;
        let closed = inner.closed;
        let slot = &mut inner.slots[role.index()];
        if closed || slot.generation != generation {
            return Err(SubscriptionWorkerRuntimeError::Invariant);
        }
        let timer = match &mut slot.timer {
            TimerState::Armed(_) => {
                let TimerState::Armed(timer) =
                    std::mem::replace(&mut slot.timer, TimerState::Empty)
                else {
                    unreachable!("matched armed deadline")
                };
                Some(timer)
            }
            TimerState::Fired => {
                slot.timer = TimerState::Empty;
                None
            }
            TimerState::Polling { clear } => {
                *clear = true;
                None
            }
            TimerState::Empty => return Err(SubscriptionWorkerRuntimeError::Invariant),
        };
        let proxy = slot.proxy_waker.take();
        drop(inner);
        drop(proxy);
        drop(timer);
        Ok(())
    }

    pub(in crate::subscription::worker::deadline) fn clear_current(
        &self,
        role: DeadlineRole,
    ) -> Result<(), SubscriptionWorkerRuntimeError> {
        let generation = {
            let inner = self.lock()?;
            let slot = &inner.slots[role.index()];
            if inner.closed
                || !matches!(
                    slot.timer,
                    TimerState::Armed(_) | TimerState::Polling { clear: false } | TimerState::Fired
                )
            {
                return Err(SubscriptionWorkerRuntimeError::Invariant);
            }
            slot.generation
        };
        self.clear(role, generation)
    }

    pub(in crate::subscription::worker::deadline) fn abandon(
        &self,
        role: DeadlineRole,
        generation: u64,
    ) {
        if self.clear(role, generation).is_err() {
            self.close();
        }
    }
}
