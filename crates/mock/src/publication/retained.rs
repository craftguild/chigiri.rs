//! Keep one precommit capture charge alive across bounded post-commit fanout.

use std::sync::Arc;

use crate::ir::FieldId;
use crate::output::OutputValue;
use crate::store::Snapshot;
use crate::value::{ValueBudget, ValueError};

use super::staging::DispatchReservation;
use super::{CapturedBatch, CapturedPublication, PublicationError, SequenceDefinition};

/// The only shared owner of one immutable publication batch and its exact
/// staging reservation. It is allocated before the store commit fence.
pub(super) struct RetainedBatch {
    batch: CapturedBatch,
    _reservation: DispatchReservation,
}

/// One indexed source item delivered without copying the captured payload.
pub(crate) struct CapturedEvent {
    capture: EventCapture,
    delivery: Option<super::source::accounting::DeliveryLease>,
    dropped: Option<u64>,
}

enum EventCapture {
    Publication {
        retained: Arc<RetainedBatch>,
        index: usize,
    },
    Sequence {
        definition: Arc<SequenceDefinition>,
        index: usize,
    },
}

impl RetainedBatch {
    pub(super) fn new(batch: CapturedBatch, reservation: DispatchReservation) -> Arc<Self> {
        Arc::new(Self {
            batch,
            _reservation: reservation,
        })
    }

    pub(super) const fn batch(&self) -> &CapturedBatch {
        &self.batch
    }

    pub(super) const fn reservation(&self) -> &DispatchReservation {
        &self._reservation
    }

    pub(super) fn event(
        retained: &Arc<Self>,
        publication: usize,
    ) -> Result<CapturedEvent, PublicationError> {
        if retained.batch.publications().get(publication).is_none() {
            return Err(PublicationError::Invariant);
        }
        Ok(CapturedEvent {
            capture: EventCapture::Publication {
                retained: Arc::clone(retained),
                index: publication,
            },
            delivery: None,
            dropped: None,
        })
    }

    pub(super) fn publication(&self, index: usize) -> Option<&CapturedPublication> {
        self.batch.publications().get(index)
    }

    #[cfg(test)]
    pub(super) fn into_parts(self) -> (CapturedBatch, DispatchReservation) {
        (self.batch, self._reservation)
    }
}

impl CapturedEvent {
    pub(crate) fn depth(&self) -> u8 {
        match &self.capture {
            EventCapture::Publication { retained, .. } => retained.batch().depth(),
            EventCapture::Sequence { .. } => 0,
        }
    }

    pub(crate) fn topic(&self) -> Option<&str> {
        match &self.capture {
            EventCapture::Publication { retained, index } => {
                retained.publication(*index).map(CapturedPublication::topic)
            }
            EventCapture::Sequence { .. } => None,
        }
    }

    pub(in crate::publication) const fn envelope_bytes() -> usize {
        size_of::<Self>()
            + size_of::<super::source::accounting::DeliveryLease>()
            + size_of::<std::time::Instant>()
    }

    pub(in crate::publication) fn source_bytes(&self) -> usize {
        match &self.capture {
            EventCapture::Publication { retained, index } => retained
                .publication(*index)
                .and_then(|publication| {
                    publication
                        .physical_bytes()
                        .checked_add(Self::envelope_bytes())
                })
                .unwrap_or(usize::MAX),
            EventCapture::Sequence { definition, index } => {
                definition.event_bytes(*index).unwrap_or(usize::MAX)
            }
        }
    }

    pub(super) fn sequence(
        definition: &Arc<SequenceDefinition>,
        index: usize,
    ) -> Result<Self, PublicationError> {
        if definition.event_bytes(index).is_none() {
            return Err(PublicationError::Invariant);
        }
        Ok(Self {
            capture: EventCapture::Sequence {
                definition: definition.clone(),
                index,
            },
            delivery: None,
            dropped: None,
        })
    }

    pub(in crate::publication) fn attach_delivery(
        &mut self,
        lease: super::source::accounting::DeliveryLease,
    ) {
        self.delivery = Some(lease);
    }

    pub(in crate::publication) fn set_dropped(&mut self, total: Option<u64>) {
        self.dropped = total;
    }

    pub(crate) const fn dropped_total(&self) -> Option<u64> {
        self.dropped
    }

    pub(crate) fn cancellation(&self) -> Option<&crate::cancellation::Cancellation> {
        self.delivery.as_ref().map(|lease| lease.cancellation())
    }
    pub(super) fn belongs_to(
        &self,
        staging: &super::staging::DispatchStaging,
        retention: &crate::events::retention::Retention,
    ) -> bool {
        match &self.capture {
            EventCapture::Publication { retained, .. } => {
                staging.owns_reservation(retained.reservation())
            }
            EventCapture::Sequence { definition, .. } => definition.belongs_to(retention),
        }
    }

    pub(crate) fn project(
        &self,
        snapshot: Snapshot,
        field: FieldId,
        budget: &mut ValueBudget,
    ) -> Result<OutputValue, ValueError> {
        if self.cancellation().is_some_and(|cancel| cancel.cancelled()) {
            return Err(ValueError("source execution cancelled"));
        }
        match &self.capture {
            EventCapture::Publication { retained, index } => retained
                .publication(*index)
                .ok_or(ValueError("publication event index is invalid"))?
                .project(snapshot, field, budget),
            EventCapture::Sequence { definition, index } => {
                definition.project(*index, snapshot, field, budget)
            }
        }
    }

    #[cfg(test)]
    pub(super) fn publication(&self) -> &CapturedPublication {
        match &self.capture {
            EventCapture::Publication { retained, index } => {
                retained.publication(*index).expect("captured event index")
            }
            EventCapture::Sequence { .. } => panic!("sequence item is not a publication"),
        }
    }

    #[cfg(test)]
    pub(super) fn shares_batch_with(&self, other: &Self) -> bool {
        match (&self.capture, &other.capture) {
            (
                EventCapture::Publication { retained: left, .. },
                EventCapture::Publication {
                    retained: right, ..
                },
            ) => Arc::ptr_eq(left, right),
            _ => false,
        }
    }
}
