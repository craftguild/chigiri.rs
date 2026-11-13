//! Copy a validated output into one schema-bound canonical immutable value.

use std::collections::BTreeMap;

use chigiri::core::{Object, Value};

mod digest;
mod records;
use digest::record_digest;

use super::meter::{DIGEST_BYTES, PAYLOAD_BYTE, SCALAR_BYTES, TAG_BYTES};
use super::{PublicationError, meter::Meter};
use crate::ir::{FieldId, MockIr};
use crate::output::{ObjectData, OutputObject, OutputValue};

#[derive(Debug, Eq, PartialEq)]
pub(super) enum CapturedValue {
    Null,
    Boolean(bool),
    Integer(i64),
    Float(u64),
    BigInt(Box<str>),
    String(Box<str>),
    List(Vec<Self>),
    Map(Vec<CapturedField>),
    Object(CapturedObject),
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct CapturedField {
    pub(super) name: Box<str>,
    pub(super) value: CapturedValue,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct CapturedObject {
    pub(super) type_name: Box<str>,
    pub(super) origin: CapturedOrigin,
    pub(super) fields: Vec<CapturedField>,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum CapturedOrigin {
    Dto,
    Stored,
    Record {
        model: Box<str>,
        public_identity: Option<Box<str>>,
        digest: [u8; DIGEST_BYTES],
    },
}

pub(super) struct CaptureWriter<'a> {
    ir: &'a MockIr,
    meter: Meter,
    retention: Option<&'a mut crate::events::retention::Reservation>,
}

impl<'a> CaptureWriter<'a> {
    pub(super) const fn new(ir: &'a MockIr, meter: Meter) -> Self {
        Self {
            ir,
            meter,
            retention: None,
        }
    }

    pub(super) fn retained(
        ir: &'a MockIr,
        meter: Meter,
        retention: &'a mut crate::events::retention::Reservation,
    ) -> Self {
        Self {
            ir,
            meter,
            retention: Some(retention),
        }
    }

    fn physical(&mut self, bytes: usize) -> Result<(), PublicationError> {
        if let Some(retention) = &mut self.retention {
            retention
                .grow(bytes)
                .map_err(|_| PublicationError::CaptureLimit)?;
        }
        Ok(())
    }

    fn reserved<T>(&mut self, len: usize) -> Result<Vec<T>, PublicationError> {
        self.physical(
            len.checked_mul(size_of::<T>())
                .ok_or(PublicationError::ArithmeticOverflow)?,
        )?;
        reserved(len)
    }

    pub(super) fn output(
        mut self,
        value: &OutputValue,
    ) -> Result<(CapturedValue, Meter), PublicationError> {
        let value = self.output_value(value)?;
        Ok((value, self.meter))
    }

    fn output_value(&mut self, value: &OutputValue) -> Result<CapturedValue, PublicationError> {
        match value {
            OutputValue::Null => self.null(),
            OutputValue::Scalar(value) => self.scalar(value),
            OutputValue::List(values) => {
                self.meter.collection(values.len())?;
                let mut captured = self.reserved(values.len())?;
                for value in values {
                    captured.push(self.output_value(value)?);
                }
                Ok(CapturedValue::List(captured))
            }
            OutputValue::Object(object) => self.object(object),
        }
    }

    fn scalar(&mut self, value: &Value) -> Result<CapturedValue, PublicationError> {
        match value {
            Value::Null => self.null(),
            Value::Boolean(value) => {
                self.meter.add(TAG_BYTES + PAYLOAD_BYTE)?;
                Ok(CapturedValue::Boolean(*value))
            }
            Value::Int(value) => {
                let value = i64::try_from(*value).map_err(|_| PublicationError::InvalidPayload)?;
                self.meter.add(TAG_BYTES + SCALAR_BYTES)?;
                Ok(CapturedValue::Integer(value))
            }
            Value::Float(value) if value.is_finite() => {
                self.meter.add(TAG_BYTES + SCALAR_BYTES)?;
                Ok(CapturedValue::Float(value.to_bits()))
            }
            Value::BigInt(value) => {
                self.meter.add(TAG_BYTES)?;
                Ok(CapturedValue::BigInt(self.text(value)?))
            }
            Value::String(value) => {
                self.meter.add(TAG_BYTES)?;
                Ok(CapturedValue::String(self.text(value)?))
            }
            Value::List(values) => {
                self.meter.collection(values.len())?;
                let mut captured = self.reserved(values.len())?;
                for value in values {
                    captured.push(self.scalar(value)?);
                }
                Ok(CapturedValue::List(captured))
            }
            Value::Object(object) => self.map(object),
            Value::Float(_) => Err(PublicationError::InvalidPayload),
        }
    }

    fn null(&mut self) -> Result<CapturedValue, PublicationError> {
        self.meter.add(TAG_BYTES + PAYLOAD_BYTE)?;
        Ok(CapturedValue::Null)
    }

    fn map(&mut self, object: &Object) -> Result<CapturedValue, PublicationError> {
        self.meter.collection(object.len())?;
        let mut fields = self.reserved(object.len())?;
        for (name, value) in object {
            fields.push(CapturedField {
                name: self.text(name)?,
                value: self.scalar(value)?,
            });
        }
        Ok(CapturedValue::Map(fields))
    }

    fn object(&mut self, object: &OutputObject) -> Result<CapturedValue, PublicationError> {
        if !std::ptr::eq(object.snapshot().ir().as_ref(), self.ir) {
            return Err(PublicationError::InvalidPayload);
        }
        self.meter.add(TAG_BYTES)?;
        let graph = self.ir.graph();
        let type_name = self.text(&graph.types()[object.ty().0].name)?;
        let (origin, fields) = match object.data() {
            ObjectData::CapturedRecord(record) => {
                if !record.belongs_to(self.ir) {
                    return Err(PublicationError::InvalidPayload);
                }
                self.record_origin(
                    record.model(),
                    object.ty(),
                    record.values(),
                    record.identity(),
                    &type_name,
                )?
            }
            ObjectData::Dto(fields) => (CapturedOrigin::Dto, self.output_fields(fields)?),
            ObjectData::Stored(fields) => (CapturedOrigin::Stored, self.output_fields(fields)?),
            ObjectData::Model(record) => {
                if !record.belongs_to(self.ir) {
                    return Err(PublicationError::InvalidPayload);
                }
                self.record_origin(
                    record.model(),
                    object.ty(),
                    record.values(),
                    record.id(),
                    &type_name,
                )?
            }
        };
        Ok(CapturedValue::Object(CapturedObject {
            type_name,
            origin,
            fields,
        }))
    }

    fn output_fields(
        &mut self,
        values: &BTreeMap<FieldId, OutputValue>,
    ) -> Result<Vec<CapturedField>, PublicationError> {
        self.meter.fields(values.len())?;
        let mut fields = self.reserved(values.len())?;
        for (field, value) in values {
            fields.push(CapturedField {
                name: self.text(&self.ir.graph().fields()[field.0].name)?,
                value: self.output_value(value)?,
            });
        }
        Ok(fields)
    }

    fn record_fields<'b>(
        &mut self,
        values: impl ExactSizeIterator<Item = (crate::ir::SlotId, &'b Value)>,
    ) -> Result<Vec<CapturedField>, PublicationError> {
        self.meter.fields(values.len())?;
        let mut fields = self.reserved(values.len())?;
        for (slot, value) in values {
            fields.push(CapturedField {
                name: self.text(&self.ir.slots()[slot.0].name)?,
                value: self.scalar(value)?,
            });
        }
        Ok(fields)
    }

    fn text(&mut self, value: &str) -> Result<Box<str>, PublicationError> {
        self.meter.add(value.len())?;
        self.physical(value.len())?;
        let mut owned = String::new();
        owned
            .try_reserve_exact(value.len())
            .map_err(|_| PublicationError::Allocation)?;
        owned.push_str(value);
        Ok(owned.into_boxed_str())
    }
}

fn reserved<T>(len: usize) -> Result<Vec<T>, PublicationError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| PublicationError::Allocation)?;
    Ok(values)
}
