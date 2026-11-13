//! Preserve validated publication call order in one capped immutable batch.

use super::capture::{CaptureWriter, CapturedValue};
use super::meter::{ENTRY_BYTES, LENGTH_BYTES, Meter, TAG_BYTES};
use super::{PublicationError, PublicationLimits};
use crate::events::retention::{Reservation, Retention};
use crate::ir::{FieldId, MockIr};
use crate::output::OutputValue;

const PUBLICATION_FRAMING: usize = ENTRY_BYTES * 2;
const BATCH_FRAMING: usize = TAG_BYTES + LENGTH_BYTES;

#[derive(Debug)]
pub(super) struct CapturedPublication {
    coordinate: Box<str>,
    additional: Vec<Box<str>>,
    topic: Box<str>,
    payload: CapturedValue,
    physical_bytes: usize,
}

impl CapturedPublication {
    pub(super) const fn physical_bytes(&self) -> usize {
        self.physical_bytes
    }
    pub(super) fn matches_coordinate(&self, coordinate: &str) -> bool {
        self.coordinate.as_ref() == coordinate
            || self
                .additional
                .iter()
                .any(|target| target.as_ref() == coordinate)
    }

    pub(super) fn coordinates(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.coordinate.as_ref()).chain(self.additional.iter().map(AsRef::as_ref))
    }
    pub(super) fn topic(&self) -> &str {
        &self.topic
    }

    pub(super) fn coordinate(&self) -> &str {
        &self.coordinate
    }

    pub(super) const fn payload(&self) -> &CapturedValue {
        &self.payload
    }
}

#[derive(Debug)]
pub(super) struct CapturedBatch {
    publications: Vec<CapturedPublication>,
    bytes: usize,
    _retention: Option<Reservation>,
    depth: u8,
}

impl CapturedBatch {
    pub(super) const fn depth(&self) -> u8 {
        self.depth
    }

    pub(super) fn publications(&self) -> &[CapturedPublication] {
        &self.publications
    }

    pub(super) const fn bytes(&self) -> usize {
        self.bytes
    }
}

pub(super) struct BatchBuilder {
    limits: PublicationLimits,
    publications: Vec<CapturedPublication>,
    bytes: usize,
    retention: Option<Reservation>,
    depth: u8,
}

impl BatchBuilder {
    #[cfg(test)]
    pub(super) fn new(limits: PublicationLimits) -> Result<Self, PublicationError> {
        Self::build(limits, None)
    }

    pub(super) fn retained(
        limits: PublicationLimits,
        retention: &Retention,
    ) -> Result<Self, PublicationError> {
        let bytes = limits
            .count()
            .checked_mul(size_of::<CapturedPublication>())
            .and_then(|bytes| {
                bytes.checked_add(
                    super::buffer::CAPTURE_STATE_BYTES
                        + size_of::<super::retained::RetainedBatch>()
                        + 2 * size_of::<usize>(),
                )
            })
            .ok_or(PublicationError::ArithmeticOverflow)?;
        let reservation = retention
            .reserve(bytes, usize::MAX)
            .map_err(|_| PublicationError::CaptureLimit)?;
        Self::build(limits, Some(reservation))
    }

    fn build(
        limits: PublicationLimits,
        retention: Option<Reservation>,
    ) -> Result<Self, PublicationError> {
        let mut publications = Vec::new();
        publications
            .try_reserve_exact(limits.count())
            .map_err(|_| PublicationError::Allocation)?;
        Ok(Self {
            limits,
            publications,
            bytes: 0,
            retention,
            depth: 0,
        })
    }

    #[cfg(test)]
    pub(super) fn push(
        &mut self,
        ir: &MockIr,
        field: FieldId,
        topic: &str,
        payload: &OutputValue,
    ) -> Result<(), PublicationError> {
        self.push_matching(ir, &[field], topic, payload)
    }

    pub(super) fn push_matching(
        &mut self,
        ir: &MockIr,
        fields: &[FieldId],
        topic: &str,
        payload: &OutputValue,
    ) -> Result<(), PublicationError> {
        let field = *fields.first().ok_or(PublicationError::InvalidCoordinate)?;
        let mut unique = std::collections::BTreeSet::new();
        for candidate in fields {
            coordinate_parts(ir, *candidate)?;
            if !unique.insert(*candidate)
                || ir.graph().fields()[candidate.0].ty.named
                    != ir.graph().fields()[field.0].ty.named
            {
                return Err(PublicationError::InvalidCoordinate);
            }
        }
        if self.publications.len() >= self.limits.count() {
            return Err(PublicationError::PublicationCount);
        }
        validate_topic(topic)?;
        let (parent, definition) = coordinate_parts(ir, field)?;
        let coordinate_len = parent
            .len()
            .checked_add(1)
            .and_then(|length| length.checked_add(definition.len()))
            .ok_or(PublicationError::ArithmeticOverflow)?;
        let mut meter = Meter::new(self.bytes, self.limits.bytes());
        let physical_before = self.retention.as_ref().map_or(0, Reservation::bytes);
        if self.publications.is_empty() {
            meter.add(BATCH_FRAMING)?;
        }
        meter.add(PUBLICATION_FRAMING)?;
        meter.add(topic.len())?;
        meter.add(coordinate_len)?;
        let mut additional = Vec::new();
        if fields.len() > 1 {
            meter.add(LENGTH_BYTES)?;
            meter.add(
                (fields.len() - 1)
                    .checked_mul(ENTRY_BYTES)
                    .ok_or(PublicationError::ArithmeticOverflow)?,
            )?;
            // Charge every additional destination string before copying any of
            // them. They select projections of one physically captured payload.
            for field in &fields[1..] {
                let (parent, name) = coordinate_parts(ir, *field)?;
                meter.add(
                    parent
                        .len()
                        .checked_add(1)
                        .and_then(|n| n.checked_add(name.len()))
                        .ok_or(PublicationError::ArithmeticOverflow)?,
                )?;
            }
            if let Some(retention) = &mut self.retention {
                let mut bytes = (fields.len() - 1)
                    .checked_mul(size_of::<Box<str>>())
                    .ok_or(PublicationError::ArithmeticOverflow)?;
                for field in &fields[1..] {
                    let (parent, name) = coordinate_parts(ir, *field)?;
                    bytes = bytes
                        .checked_add(parent.len() + 1 + name.len())
                        .ok_or(PublicationError::ArithmeticOverflow)?;
                }
                retention
                    .grow(bytes)
                    .map_err(|_| PublicationError::CaptureLimit)?;
            }
            additional
                .try_reserve_exact(fields.len() - 1)
                .map_err(|_| PublicationError::Allocation)?;
            for field in &fields[1..] {
                additional.push(owned_coordinate(ir, *field)?);
            }
        }
        if let Some(retention) = &mut self.retention {
            retention
                .grow(topic.len() + coordinate_len)
                .map_err(|_| PublicationError::CaptureLimit)?;
        }
        let writer = match &mut self.retention {
            Some(retention) => CaptureWriter::retained(ir, meter, retention),
            None => CaptureWriter::new(ir, meter),
        };
        let (payload, meter) = writer.output(payload)?;
        let physical_bytes = self
            .retention
            .as_ref()
            .map_or(meter.used() - self.bytes, |reservation| {
                reservation.bytes() - physical_before + size_of::<CapturedPublication>()
            });
        let topic = owned(topic)?;
        let mut coordinate = String::new();
        coordinate
            .try_reserve_exact(coordinate_len)
            .map_err(|_| PublicationError::Allocation)?;
        coordinate.push_str(parent);
        coordinate.push('.');
        coordinate.push_str(definition);
        self.publications.push(CapturedPublication {
            coordinate: coordinate.into_boxed_str(),
            additional,
            topic,
            payload,
            physical_bytes,
        });
        self.bytes = meter.used();
        Ok(())
    }

    pub(super) fn seal(self) -> Option<CapturedBatch> {
        (!self.publications.is_empty()).then_some(CapturedBatch {
            publications: self.publications,
            bytes: self.bytes,
            _retention: self.retention,
            depth: self.depth,
        })
    }

    pub(super) fn set_depth(&mut self, depth: u8) {
        self.depth = depth;
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.publications.len()
    }

    #[cfg(test)]
    pub(super) const fn bytes(&self) -> usize {
        self.bytes
    }
}

pub(super) fn owned_coordinate(ir: &MockIr, field: FieldId) -> Result<Box<str>, PublicationError> {
    let (parent, definition) = coordinate_parts(ir, field)?;
    let length = parent
        .len()
        .checked_add(1)
        .and_then(|length| length.checked_add(definition.len()))
        .ok_or(PublicationError::ArithmeticOverflow)?;
    let mut coordinate = String::new();
    coordinate
        .try_reserve_exact(length)
        .map_err(|_| PublicationError::Allocation)?;
    coordinate.push_str(parent);
    coordinate.push('.');
    coordinate.push_str(definition);
    Ok(coordinate.into_boxed_str())
}

pub(super) fn coordinate_parts(
    ir: &MockIr,
    field: FieldId,
) -> Result<(&str, &str), PublicationError> {
    let graph = ir.graph();
    let subscription = graph
        .subscription()
        .ok_or(PublicationError::InvalidCoordinate)?;
    let definition = graph
        .fields()
        .get(field.0)
        .filter(|field| field.parent == subscription)
        .ok_or(PublicationError::InvalidCoordinate)?;
    Ok((&graph.types()[subscription.0].name, &definition.name))
}

pub(super) fn validate_topic(topic: &str) -> Result<(), PublicationError> {
    if topic.is_empty()
        || topic.len() > super::limits::MAX_TOPIC_BYTES
        || topic.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(PublicationError::InvalidTopic);
    }
    Ok(())
}

pub(super) fn owned(value: &str) -> Result<Box<str>, PublicationError> {
    let mut owned = String::new();
    owned
        .try_reserve_exact(value.len())
        .map_err(|_| PublicationError::Allocation)?;
    owned.push_str(value);
    Ok(owned.into_boxed_str())
}
