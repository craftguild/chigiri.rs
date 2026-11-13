//! Close one complete request's capture before reserving dispatch capacity.

use std::sync::{Arc, Mutex};

use super::batch::BatchBuilder;
use super::retained::RetainedBatch;
use super::staging::DispatchStaging;
use super::{PublicationError, PublicationLimits};
use crate::ir::{FieldId, MockIr};
use crate::output::OutputValue;

struct State {
    builder: Option<BatchBuilder>,
    failed: bool,
    can_publish: bool,
}

pub(super) const CAPTURE_STATE_BYTES: usize = size_of::<Mutex<State>>() + 2 * size_of::<usize>();

/// Shared only by a write candidate and its short VMs, never by a staged event.
#[derive(Clone)]
pub(crate) struct PublicationBuffer {
    state: Arc<Mutex<State>>,
    owner: PublicationOwner,
}

/// Fixed server metadata, captured before building any publication. It is never
/// retained by a staged batch and cannot be synthesized outside this module.
#[derive(Clone)]
pub(crate) struct PublicationOwner {
    ir: Arc<MockIr>,
    staging: Arc<()>,
    retention: crate::events::retention::Retention,
}

/// Move-only original capture and its precommit queue/byte reservation.
pub(crate) struct PreparedPublications {
    pub(super) retained: Arc<RetainedBatch>,
}

impl PublicationOwner {
    pub(super) fn new(
        ir: Arc<MockIr>,
        staging: Arc<()>,
        retention: crate::events::retention::Retention,
    ) -> Self {
        Self {
            ir,
            staging,
            retention,
        }
    }

    pub(crate) fn buffer(
        &self,
        limits: PublicationLimits,
    ) -> Result<PublicationBuffer, PublicationError> {
        self.buffer_with_depth(limits, Some(0))
    }

    pub(crate) fn consumer_buffer(
        &self,
        limits: PublicationLimits,
        parent_depth: u8,
    ) -> Result<PublicationBuffer, PublicationError> {
        self.buffer_with_depth(
            limits,
            parent_depth
                .checked_add(1)
                .filter(|depth| *depth <= crate::api::consumers::MAX_DEPTH),
        )
    }

    fn buffer_with_depth(
        &self,
        limits: PublicationLimits,
        depth: Option<u8>,
    ) -> Result<PublicationBuffer, PublicationError> {
        let mut builder = BatchBuilder::retained(limits, &self.retention)?;
        builder.set_depth(depth.unwrap_or(0));
        Ok(PublicationBuffer {
            state: Arc::new(Mutex::new(State {
                builder: Some(builder),
                failed: false,
                can_publish: depth.is_some(),
            })),
            owner: self.clone(),
        })
    }
}

impl PublicationBuffer {
    /// Callers first validate the exact coordinate's output type and provenance.
    /// Any rejected capture permanently invalidates the entire request buffer.
    #[cfg(test)]
    pub(crate) fn push(
        &self,
        ir: &MockIr,
        field: FieldId,
        topic: &str,
        payload: &OutputValue,
    ) -> Result<(), PublicationError> {
        self.push_matching(ir, &[field], topic, payload)
    }

    pub(crate) fn push_matching(
        &self,
        ir: &MockIr,
        fields: &[FieldId],
        topic: &str,
        payload: &OutputValue,
    ) -> Result<(), PublicationError> {
        let mut state = self.state.lock().map_err(|_| PublicationError::Invariant)?;
        if !std::ptr::eq(self.owner.ir.as_ref(), ir) {
            state.failed = true;
            return Err(PublicationError::ForeignOwner);
        }
        if state.failed {
            return Err(PublicationError::InvalidPayload);
        }
        if !state.can_publish {
            state.failed = true;
            return Err(PublicationError::DepthExceeded);
        }
        let result = state
            .builder
            .as_mut()
            .ok_or(PublicationError::StagingClosed)
            .and_then(|builder| builder.push_matching(ir, fields, topic, payload));
        state.failed |= result.is_err();
        result
    }

    /// Close capture once, then reserve outside the request/store/cancellation fence.
    pub(crate) fn prepare(
        &self,
        staging: &DispatchStaging,
    ) -> Result<Option<PreparedPublications>, PublicationError> {
        if !staging.owns_capture(&self.owner.staging) {
            self.state
                .lock()
                .map_err(|_| PublicationError::Invariant)?
                .failed = true;
            return Err(PublicationError::ForeignOwner);
        }
        let (builder, failed) = {
            let mut state = self.state.lock().map_err(|_| PublicationError::Invariant)?;
            (state.builder.take(), state.failed)
        };
        let builder = builder.ok_or(PublicationError::StagingClosed)?;
        if failed {
            return Err(PublicationError::InvalidPayload);
        }
        let Some(batch) = builder.seal() else {
            return Ok(None);
        };
        match staging.try_reserve(batch.bytes()) {
            Ok(reservation) => Ok(Some(PreparedPublications {
                retained: RetainedBatch::new(batch, reservation),
            })),
            Err(error) => {
                // A later attempt cannot restore admission or discard this hard failure.
                self.state
                    .lock()
                    .map_err(|_| PublicationError::Invariant)?
                    .failed = true;
                Err(error)
            }
        }
    }
}
