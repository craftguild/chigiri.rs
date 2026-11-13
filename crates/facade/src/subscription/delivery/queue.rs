//! FIFO ownership for wholly unsent, therefore evictable, encoded events.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use super::{EventByteCharge, InFlightByteCharge, MonotonicInstant};

/// One wholly encoded event that has not emitted any byte.
#[derive(Debug)]
pub(crate) struct QueuedEvent<T> {
    admitted_at: MonotonicInstant,
    payload: T,
    charge: EventByteCharge,
}

impl<T> QueuedEvent<T> {
    pub(crate) fn new(admitted_at: MonotonicInstant, payload: T, charge: EventByteCharge) -> Self {
        Self {
            admitted_at,
            payload,
            charge,
        }
    }

    pub(crate) const fn admitted_at(&self) -> MonotonicInstant {
        self.admitted_at
    }

    pub(crate) fn bytes(&self) -> usize {
        self.charge.bytes()
    }

    pub(super) fn into_in_flight_with<F, E>(
        self,
        now: MonotonicInstant,
        dropped_total: u64,
        finalize: impl FnOnce(T, u64) -> Result<F, E>,
    ) -> Result<(InFlightSlot<F>, InFlightSend<F>), E> {
        let payload = finalize(self.payload, dropped_total)?;
        let bytes = self.charge.bytes();
        let progress = Arc::new(Mutex::new(SendProgress {
            total: bytes,
            accepted: 0,
            last_progress: now,
        }));
        let entry = Arc::new(InFlightEntry {
            payload,
            charge: self.charge.into_in_flight(),
            progress: Arc::clone(&progress),
            sender_released: AtomicBool::new(false),
        });
        Ok((
            InFlightSlot {
                entry: Arc::clone(&entry),
            },
            InFlightSend {
                entry,
                progress: Some(InFlightProgress { state: progress }),
            },
        ))
    }
}

#[derive(Debug)]
struct InFlightEntry<T> {
    payload: T,
    charge: InFlightByteCharge,
    progress: Arc<Mutex<SendProgress>>,
    sender_released: AtomicBool,
}

#[derive(Clone, Copy, Debug)]
struct SendProgress {
    total: usize,
    accepted: usize,
    last_progress: MonotonicInstant,
}

/// Controller-owned identity for the one non-evictable event.
#[derive(Debug)]
pub(crate) struct InFlightSlot<T> {
    entry: Arc<InFlightEntry<T>>,
}

impl<T> InFlightSlot<T> {
    pub(crate) fn bytes(&self) -> usize {
        self.entry.charge.bytes()
    }

    pub(crate) fn progress(&self) -> Result<(usize, MonotonicInstant), EventQueueError> {
        let progress = lock_progress(&self.entry.progress)?;
        Ok((progress.accepted, progress.last_progress))
    }

    pub(crate) fn matches(&self, progress: &InFlightProgress) -> bool {
        Arc::ptr_eq(&self.entry.progress, &progress.state)
    }

    pub(crate) fn sender_released(&self) -> bool {
        self.entry.sender_released.load(Ordering::Acquire)
    }
}

/// Move-only immutable finalized payload owner for zero-copy bridge handoff.
#[derive(Debug)]
pub(crate) struct InFlightSend<T> {
    entry: Arc<InFlightEntry<T>>,
    progress: Option<InFlightProgress>,
}

impl<T> InFlightSend<T> {
    pub(crate) fn payload(&self) -> &T {
        &self.entry.payload
    }

    pub(crate) fn bytes(&self) -> usize {
        self.entry.charge.bytes()
    }

    /// Moves out the one writer-progress authority.
    pub(crate) fn take_progress(&mut self) -> Option<InFlightProgress> {
        self.progress.take()
    }
}

impl<T> Drop for InFlightSend<T> {
    fn drop(&mut self) {
        self.entry.sender_released.store(true, Ordering::Release);
    }
}

/// Progress-only authority that does not retain payload or byte charges.
#[derive(Debug)]
pub(crate) struct InFlightProgress {
    state: Arc<Mutex<SendProgress>>,
}

impl InFlightProgress {
    #[cfg(feature = "subscription-worker")]
    pub(crate) fn no_progress_deadline(
        &self,
        timeout: std::time::Duration,
    ) -> Result<Option<MonotonicInstant>, EventQueueError> {
        let progress = lock_progress(&self.state)?;
        if progress.accepted == progress.total {
            return Ok(None);
        }
        progress
            .last_progress
            .checked_add(timeout)
            .map(Some)
            .map_err(|_| EventQueueError::Accounting)
    }

    pub(crate) fn accepted_bytes(&self) -> Result<usize, EventQueueError> {
        Ok(lock_progress(&self.state)?.accepted)
    }

    #[cfg(test)]
    pub(crate) fn last_progress(&self) -> Result<MonotonicInstant, EventQueueError> {
        Ok(lock_progress(&self.state)?.last_progress)
    }

    /// Records bytes accepted by the bridge; zero never resets progress.
    pub(crate) fn accept(
        &mut self,
        bytes: usize,
        now: MonotonicInstant,
    ) -> Result<bool, EventQueueError> {
        let mut progress = lock_progress(&self.state)?;
        let accepted = progress
            .accepted
            .checked_add(bytes)
            .ok_or(EventQueueError::Accounting)?;
        if accepted > progress.total {
            return Err(EventQueueError::Accounting);
        }
        progress.accepted = accepted;
        if bytes > 0 {
            progress.last_progress = now;
        }
        Ok(progress.accepted == progress.total)
    }
}

fn lock_progress(
    progress: &Mutex<SendProgress>,
) -> Result<MutexGuard<'_, SendProgress>, EventQueueError> {
    progress.lock().map_err(|_| EventQueueError::Accounting)
}

/// Ordered queue containing only whole event entries.
#[derive(Debug, Default)]
pub(crate) struct EventQueue<T> {
    entries: VecDeque<QueuedEvent<T>>,
    bytes: usize,
}

impl<T> EventQueue<T> {
    pub(crate) const fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            bytes: 0,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) const fn bytes(&self) -> usize {
        self.bytes
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn oldest_admitted_at(&self) -> Option<MonotonicInstant> {
        self.entries.front().map(QueuedEvent::admitted_at)
    }

    pub(crate) fn push(&mut self, event: QueuedEvent<T>) -> Result<(), EventQueueError> {
        self.bytes = self
            .bytes
            .checked_add(event.bytes())
            .ok_or(EventQueueError::Accounting)?;
        self.entries.push_back(event);
        Ok(())
    }

    pub(crate) fn evict_oldest(&mut self) -> Result<Option<QueuedEvent<T>>, EventQueueError> {
        self.pop_front()
    }

    pub(super) fn take_oldest(&mut self) -> Result<Option<QueuedEvent<T>>, EventQueueError> {
        self.pop_front()
    }

    fn pop_front(&mut self) -> Result<Option<QueuedEvent<T>>, EventQueueError> {
        let Some(event) = self.entries.pop_front() else {
            return Ok(None);
        };
        let Some(bytes) = self.bytes.checked_sub(event.bytes()) else {
            self.entries.push_front(event);
            return Err(EventQueueError::Accounting);
        };
        self.bytes = bytes;
        Ok(Some(event))
    }
}

/// Internal queue accounting failed and the subscription must terminate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EventQueueError {
    Accounting,
}
