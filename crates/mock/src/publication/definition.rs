//! Immutable sequence outputs retain no store, request, Luau value or closure.

use std::sync::Arc;

use super::batch::owned_coordinate;
use super::capture::{CaptureWriter, CapturedValue};
use super::meter::Meter;
use super::{CapturedEvent, PublicationError};
use crate::api::subscriptions as contract;
use crate::events::SourceLimits;
use crate::events::retention::{Reservation, Retention};
use crate::ir::{FieldId, MockIr};
use crate::output::OutputValue;
use crate::store::Snapshot;
use crate::value::{ValueBudget, ValueError};

#[derive(Debug)]
pub(crate) struct SequenceDefinition {
    coordinate: Box<str>,
    values: Vec<CapturedValue>,
    event_bytes: Vec<usize>,
    interval_ms: u64,
    looping: bool,
    reservation: Reservation,
}

pub(crate) struct DefinitionBuilder {
    definition: SequenceDefinition,
    count: usize,
    limits: SourceLimits,
    retention: Retention,
    meter: Meter,
}

impl DefinitionBuilder {
    pub(crate) fn new(
        ir: &MockIr,
        field: FieldId,
        count: usize,
        interval_ms: u64,
        looping: bool,
        limits: SourceLimits,
        retention: &Retention,
    ) -> Result<Self, PublicationError> {
        if !(1..=contract::MAX_SEQUENCE_VALUES).contains(&count)
            || !(1..=contract::MAX_INTERVAL_MS).contains(&interval_ms)
        {
            return Err(PublicationError::InvalidPayload);
        }
        let graph = ir.graph();
        let field = graph
            .fields()
            .get(field.0)
            .filter(|field| Some(field.parent) == graph.subscription())
            .ok_or(PublicationError::InvalidCoordinate)?;
        let coordinate_bytes = graph.types()[field.parent.0]
            .name
            .len()
            .checked_add(1)
            .and_then(|bytes| bytes.checked_add(field.name.len()))
            .ok_or(PublicationError::ArithmeticOverflow)?;
        let bytes = count
            .checked_mul(size_of::<CapturedValue>() + size_of::<usize>())
            .and_then(|bytes| bytes.checked_add(coordinate_bytes))
            .and_then(|bytes| bytes.checked_add(size_of::<SequenceDefinition>()))
            .and_then(|bytes| bytes.checked_add(2 * size_of::<usize>()))
            .ok_or(PublicationError::ArithmeticOverflow)?;
        let reservation = retention
            .reserve(bytes, limits.max_sequence_bytes())
            .map_err(|_| PublicationError::CaptureLimit)?;
        let mut values = Vec::new();
        let mut event_bytes = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| PublicationError::Allocation)?;
        event_bytes
            .try_reserve_exact(count)
            .map_err(|_| PublicationError::Allocation)?;
        let field = graph.types()[field.parent.0].fields[&field.name];
        Ok(Self {
            definition: SequenceDefinition {
                coordinate: owned_coordinate(ir, field)?,
                values,
                event_bytes,
                interval_ms,
                looping,
                reservation,
            },
            count,
            limits,
            retention: retention.clone(),
            meter: Meter::new(0, limits.max_sequence_bytes()),
        })
    }

    pub(crate) fn push(
        &mut self,
        ir: &MockIr,
        output: &OutputValue,
    ) -> Result<(), PublicationError> {
        if self.definition.values.len() >= self.count {
            return Err(PublicationError::InvalidPayload);
        }
        let before = self.definition.reservation.bytes();
        let (captured, meter) =
            CaptureWriter::retained(ir, self.meter, &mut self.definition.reservation)
                .output(output)?;
        let bytes = self
            .definition
            .reservation
            .bytes()
            .checked_sub(before)
            .and_then(|bytes| bytes.checked_add(size_of::<CapturedValue>()))
            .and_then(|bytes| bytes.checked_add(CapturedEvent::envelope_bytes()))
            .ok_or(PublicationError::ArithmeticOverflow)?;
        if bytes > self.limits.max_queued_bytes() {
            return Err(PublicationError::CaptureLimit);
        }
        self.meter = meter;
        self.definition.values.push(captured);
        self.definition.event_bytes.push(bytes);
        Ok(())
    }

    pub(crate) fn seal(self) -> Result<Arc<SequenceDefinition>, PublicationError> {
        if self.definition.values.len() != self.count {
            return Err(PublicationError::InvalidPayload);
        }
        let event_bytes = self.definition.event_bytes.iter().copied().max().unwrap();
        if !self.retention.fits(event_bytes) {
            return Err(PublicationError::CaptureLimit);
        }
        Ok(Arc::new(self.definition))
    }
}

impl SequenceDefinition {
    pub(super) fn accepts(&self, ir: &MockIr, field: FieldId) -> bool {
        super::batch::coordinate_parts(ir, field).is_ok_and(|(parent, name)| {
            super::projection::same_coordinate(&self.coordinate, parent, name)
        })
    }

    pub(crate) const fn interval_ms(&self) -> u64 {
        self.interval_ms
    }

    pub(crate) const fn looping(&self) -> bool {
        self.looping
    }

    pub(crate) fn len(&self) -> usize {
        self.values.len()
    }

    pub(crate) fn bytes(&self) -> usize {
        self.reservation.bytes()
    }

    pub(crate) fn event_bytes(&self, index: usize) -> Option<usize> {
        self.event_bytes.get(index).copied()
    }

    pub(crate) fn belongs_to(&self, retention: &Retention) -> bool {
        self.reservation.belongs_to(retention)
    }

    pub(crate) fn project(
        &self,
        index: usize,
        snapshot: Snapshot,
        field: FieldId,
        budget: &mut ValueBudget,
    ) -> Result<OutputValue, ValueError> {
        super::projection::root(
            &self.coordinate,
            self.values
                .get(index)
                .ok_or(ValueError("invalid sequence position"))?,
            snapshot,
            field,
            budget,
        )
    }
}
