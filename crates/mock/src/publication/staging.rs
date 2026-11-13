//! Move precharged immutable batches through a fixed server-local queue.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

#[cfg(test)]
mod inspection;
mod reservation;

use super::limits::{STAGING_BATCHES, STAGING_BYTES};
use super::retained::RetainedBatch;
use super::{CapturedBatch, CommitSequence, PublicationError, TopicIndex};

const QUEUE_FRAMING: usize = 16;

struct Inner {
    sequence_owner: Arc<()>,
    reservation_owner: Arc<()>,
    usage: Arc<Usage>,
    state: Mutex<State>,
}

struct State {
    closed: bool,
    pressure: Option<usize>,
    queue: VecDeque<StagedBatch>,
}

struct Usage {
    values: Mutex<UsageValues>,
}

struct UsageValues {
    slots: usize,
    bytes: usize,
}

pub(crate) struct DispatchStaging {
    inner: Arc<Inner>,
}

pub(super) struct DispatchReservation {
    owner: Arc<()>,
    usage: Arc<Usage>,
    bytes: usize,
    batch_bytes: usize,
}

pub(super) struct StagedBatch {
    retained: Arc<RetainedBatch>,
    sequence: CommitSequence,
}

#[cfg(test)]
pub(super) struct StageFailure {
    pub(super) error: PublicationError,
    pub(super) reservation: DispatchReservation,
    pub(super) batch: CapturedBatch,
    pub(super) sequence: CommitSequence,
}

pub(super) struct RetainedStageFailure {
    pub(super) _error: PublicationError,
    pub(super) retained: Arc<RetainedBatch>,
    pub(super) sequence: CommitSequence,
}

pub(super) struct StagedCandidate {
    pub(super) retained: Arc<RetainedBatch>,
    pub(super) sequence: u64,
    pub(super) publication: usize,
}

#[derive(Clone, Copy)]
pub(super) struct FrontBatch {
    pub(super) sequence: u64,
    pub(super) publications: usize,
}

impl DispatchStaging {
    pub(super) fn new(topics: &TopicIndex) -> Result<Self, PublicationError> {
        let mut queue = VecDeque::new();
        queue
            .try_reserve_exact(STAGING_BATCHES)
            .map_err(|_| PublicationError::Allocation)?;
        Ok(Self {
            inner: Arc::new(Inner {
                sequence_owner: topics.owner(),
                reservation_owner: Arc::new(()),
                usage: Arc::new(Usage {
                    values: Mutex::new(UsageValues { slots: 0, bytes: 0 }),
                }),
                state: Mutex::new(State {
                    closed: false,
                    pressure: None,
                    queue,
                }),
            }),
        })
    }

    pub(super) fn capture_owner(&self) -> Arc<()> {
        Arc::clone(&self.inner.reservation_owner)
    }

    pub(super) fn owns_capture(&self, owner: &Arc<()>) -> bool {
        Arc::ptr_eq(&self.inner.reservation_owner, owner)
    }

    pub(super) fn owns_reservation(&self, reservation: &DispatchReservation) -> bool {
        self.owns_capture(&reservation.owner)
    }

    pub(super) fn validate_capture(
        &self,
        reservation: &DispatchReservation,
        batch: &CapturedBatch,
    ) -> Result<(), PublicationError> {
        if !self.owns_capture(&reservation.owner) {
            return Err(PublicationError::ForeignOwner);
        }
        if reservation.batch_bytes != batch.bytes() {
            return Err(PublicationError::Invariant);
        }
        Ok(())
    }

    pub(super) fn try_reserve(
        &self,
        batch_bytes: usize,
    ) -> Result<DispatchReservation, PublicationError> {
        let bytes = QUEUE_FRAMING
            .checked_add(batch_bytes)
            .ok_or(PublicationError::ArithmeticOverflow)?;
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| PublicationError::Invariant)?;
        if state.closed {
            return Err(PublicationError::StagingClosed);
        }
        let mut usage = self
            .inner
            .usage
            .values
            .lock()
            .map_err(|_| PublicationError::Invariant)?;
        let slots = usage
            .slots
            .checked_add(1)
            .ok_or(PublicationError::ArithmeticOverflow)?;
        let total = usage
            .bytes
            .checked_add(bytes)
            .ok_or(PublicationError::ArithmeticOverflow)?;
        if slots > STAGING_BATCHES || total > STAGING_BYTES {
            if bytes <= STAGING_BYTES {
                state.pressure = Some(state.pressure.unwrap_or(0).max(bytes));
            }
            return Err(PublicationError::StagingCapacity);
        }
        usage.slots = slots;
        usage.bytes = total;
        state.pressure = None;
        Ok(DispatchReservation {
            owner: Arc::clone(&self.inner.reservation_owner),
            usage: Arc::clone(&self.inner.usage),
            bytes,
            batch_bytes,
        })
    }

    #[cfg(test)]
    pub(super) fn stage(
        &self,
        reservation: DispatchReservation,
        batch: CapturedBatch,
        sequence: CommitSequence,
    ) -> Result<(), StageFailure> {
        let retained = RetainedBatch::new(batch, reservation);
        self.stage_retained(retained, sequence).map_err(|failure| {
            let retained = Arc::try_unwrap(failure.retained)
                .unwrap_or_else(|_| unreachable!("fresh retained batch has one owner"));
            let (batch, reservation) = retained.into_parts();
            StageFailure {
                error: failure._error,
                reservation,
                batch,
                sequence: failure.sequence,
            }
        })
    }

    pub(super) fn stage_retained(
        &self,
        retained: Arc<RetainedBatch>,
        sequence: CommitSequence,
    ) -> Result<(), RetainedStageFailure> {
        let error = if !sequence.same_owner(&self.inner.sequence_owner) {
            Some(PublicationError::ForeignOwner)
        } else {
            self.validate_capture(retained.reservation(), retained.batch())
                .err()
        };
        if let Some(error) = error {
            return Err(RetainedStageFailure {
                _error: error,
                retained,
                sequence,
            });
        }
        let mut state = match self.inner.state.lock() {
            Ok(state) => state,
            Err(_) => {
                return Err(RetainedStageFailure {
                    _error: PublicationError::Invariant,
                    retained,
                    sequence,
                });
            }
        };
        if state.closed || state.queue.len() >= STAGING_BATCHES {
            let error = if state.closed {
                PublicationError::StagingClosed
            } else {
                PublicationError::Invariant
            };
            drop(state);
            return Err(RetainedStageFailure {
                _error: error,
                retained,
                sequence,
            });
        }
        state.queue.push_back(StagedBatch { retained, sequence });
        Ok(())
    }

    pub(super) fn candidate_after(
        &self,
        sequence: u64,
        publication: usize,
    ) -> Result<Option<StagedCandidate>, PublicationError> {
        let state = self
            .inner
            .state
            .lock()
            .map_err(|_| PublicationError::Invariant)?;
        for staged in &state.queue {
            let current = staged.sequence.value();
            if current < sequence {
                continue;
            }
            let index = if current == sequence { publication } else { 0 };
            if index < staged.retained.batch().publications().len() {
                return Ok(Some(StagedCandidate {
                    retained: Arc::clone(&staged.retained),
                    sequence: current,
                    publication: index,
                }));
            }
        }
        Ok(None)
    }

    pub(super) fn front(&self) -> Result<Option<FrontBatch>, PublicationError> {
        let state = self
            .inner
            .state
            .lock()
            .map_err(|_| PublicationError::Invariant)?;
        Ok(state.queue.front().map(|staged| FrontBatch {
            sequence: staged.sequence.value(),
            publications: staged.retained.batch().publications().len(),
        }))
    }

    pub(super) fn pop_front(&self, sequence: u64) -> Result<Option<StagedBatch>, PublicationError> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| PublicationError::Invariant)?;
        if state
            .queue
            .front()
            .is_some_and(|staged| staged.sequence.value() == sequence)
        {
            Ok(state.queue.pop_front())
        } else {
            Ok(None)
        }
    }

    pub(super) fn pressured(&self) -> Result<bool, PublicationError> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| PublicationError::Invariant)?;
        let Some(needed) = state.pressure else {
            return Ok(false);
        };
        let usage = self
            .inner
            .usage
            .values
            .lock()
            .map_err(|_| PublicationError::Invariant)?;
        let recovered = usage.slots < STAGING_BATCHES
            && usage
                .bytes
                .checked_add(needed)
                .is_some_and(|bytes| bytes <= STAGING_BYTES);
        if recovered {
            state.pressure = None;
        }
        Ok(!recovered)
    }

    pub(crate) fn close(&self) -> Result<(), PublicationError> {
        let retired = {
            let mut state = self
                .inner
                .state
                .lock()
                .map_err(|_| PublicationError::Invariant)?;
            state.closed = true;
            std::mem::take(&mut state.queue)
        };
        drop(retired);
        Ok(())
    }
}

impl DispatchReservation {
    #[cfg(test)]
    pub(super) const fn bytes(&self) -> usize {
        self.bytes
    }
}

impl StagedBatch {
    #[cfg(test)]
    pub(super) fn batch(&self) -> &CapturedBatch {
        self.retained.batch()
    }

    #[cfg(test)]
    pub(super) const fn sequence(&self) -> &CommitSequence {
        &self.sequence
    }
}
