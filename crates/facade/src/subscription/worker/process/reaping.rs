//! Fair process polling outside registration and supervisor locks.

use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use super::*;
use crate::subscription::worker::completion::WorkerCompletionOutcome;

pub(super) fn run_reaper(shared: Arc<ProcessShared>) {
    loop {
        let observed = lock_process(&shared).epoch;
        poll_registrations(&shared);
        supervisors::poll_all(&shared);
        let mut state = lock_process(&shared);
        if state.status == ProcessStatus::Draining
            && state.registrations.is_empty()
            && state.supervisors.is_empty()
        {
            state.status = ProcessStatus::Stopped;
            let waker = state.shutdown_waker.take();
            drop(state);
            if let Some(waker) = waker {
                waker.wake();
            }
            shared.signal.notify_all();
            #[cfg(test)]
            REAPER_THREADS_EXITED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return;
        }
        if state.epoch == observed {
            state = match shared.signal.wait(state) {
                Ok(state) => state,
                Err(poisoned) => {
                    let mut state = poisoned.into_inner();
                    state.poisoned = true;
                    state
                }
            };
        }
        drop(state);
    }
}

fn poll_registrations(shared: &Arc<ProcessShared>) {
    let registration_count = lock_process(shared).registrations.len();
    for _ in 0..registration_count {
        let Some(registration) = next_registration(shared) else {
            break;
        };
        // A ready join does not necessarily generate another process wake.
        // Visit this finite snapshot before sleeping, including records behind
        // Pending joins. Restored Pending records cannot extend this pass.
        let budget = registration.retired_count();
        if budget == 0 {
            unregister_if_drained(&registration);
            continue;
        }
        let waker = Waker::from(Arc::clone(shared.wake.get().expect("process wake")));
        let mut context = Context::from_waker(&waker);
        for _ in 0..budget {
            let Some((slot, mut record)) = registration.take_retired() else {
                break;
            };
            match record.poll(&mut context) {
                Poll::Pending => slot.restore_pending(record),
                Poll::Ready(outcome) => {
                    if outcome == WorkerCompletionOutcome::Poisoned {
                        mark_process_poisoned(&Arc::downgrade(shared));
                    }
                    slot.finish_polled(record, outcome);
                }
            }
        }
    }
}

fn next_registration(shared: &ProcessShared) -> Option<Arc<WorkerRegistration>> {
    let mut state = lock_process(shared);
    let next = state.cursor.and_then(|cursor| {
        state
            .registrations
            .range((
                std::ops::Bound::Excluded(cursor),
                std::ops::Bound::Unbounded,
            ))
            .next()
            .map(|(id, registration)| (*id, Arc::clone(registration)))
    });
    let (id, registration) = next.or_else(|| {
        state
            .registrations
            .first_key_value()
            .map(|(id, registration)| (*id, Arc::clone(registration)))
    })?;
    state.cursor = Some(id);
    Some(registration)
}

#[cfg(test)]
mod tests;
