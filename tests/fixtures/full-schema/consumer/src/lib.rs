#![forbid(unsafe_code)]
//! Exercises the complete synthetic SDK with independent domain and local types.

mod bindings;
mod types;

pub use types::Types;

/// String-backed fixture scalar; it does not implement application scalar semantics.
#[derive(Clone, Debug, PartialEq)]
pub struct ScalarValue(pub String);

impl chigiri::Scalar for ScalarValue {
    fn parse(value: chigiri::Value) -> Result<Self, chigiri::ScalarCoercionError> {
        match value {
            chigiri::Value::String(value) => Ok(Self(value)),
            _ => Err(chigiri::ScalarCoercionError::public(
                "expected corpus string",
            )),
        }
    }

    fn serialize(&self) -> chigiri::FieldResult<chigiri::Value> {
        Ok(chigiri::Value::String(self.0.clone()))
    }
}

/// A retained domain value supplies fields through ordinary resolver methods.
pub struct SampleMeasurement {
    pub value: String,
    pub unit: domain::UnitKind,
    pub precision: i32,
}

/// Resolves the SDL-defined Measurement without an output DTO.
pub struct MeasurementFields;

impl contract::MeasurementResolver<(), Types, chigiri::FieldError> for MeasurementFields {
    async fn value<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        measurement: &'a SampleMeasurement,
    ) -> chigiri::FieldResult<ScalarValue> {
        Ok(ScalarValue(measurement.value.clone()))
    }
    async fn unit<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        measurement: &'a SampleMeasurement,
    ) -> chigiri::FieldResult<domain::UnitKind> {
        Ok(measurement.unit)
    }
    async fn precision<'a>(
        &'a self,
        _: chigiri::FieldContext<'a, ()>,
        measurement: &'a SampleMeasurement,
    ) -> chigiri::FieldResult<i32> {
        Ok(measurement.precision)
    }
}

#[cfg(test)]
mod tests;
