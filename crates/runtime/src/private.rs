//! Type-erased ABI used only by generated code and the executor.

#[cfg(test)]
mod tests;

use std::fmt;

use chigiri_core::executable::{CoercedInput, ScalarCoercionError};
use chigiri_core::{FieldError, FieldResult, Value};

use crate::{Id, Input};

pub use crate::execution::classification::SelectedHttpOperation;
pub use crate::execution::{
    OneShotOperationKind, PreparedOneShotOperation, PreparedWebSocketOperation,
    WebSocketOperationKind,
};

pub use crate::subscription::{
    ErasedSubscription, ErasedSubscriptionArguments, ErasedSubscriptionEvent, HostSourceDelivery,
    HostSubscriptionObservation, PreparedSubscription, SubscriptionEventResult,
    SubscriptionObservation, SubscriptionObservationReceipt, SubscriptionResolverDispatch,
    SubscriptionResolverGroupDispatch, SubscriptionSession, SubscriptionSourceState,
    SubscriptionStart, SubscriptionTerminalResponse, SubscriptionTermination,
    erase_subscription_source,
};

pub use chigiri_core::executor::{
    ArgumentValues, BoxFuture, DispatchAttempt, ResolvedObject, ResolvedValue, ResolverDispatch,
    ResolverSchemaFingerprint,
};

/// Common type-erased dispatch boundary for generated object adapters.
#[doc(hidden)]
pub trait ResolverGroupDispatch<C>: Send + Sync + 'static {
    /// Returns the object's index in schema source order.
    fn object_index(&self) -> usize;

    /// Dispatches a field resolver on the object.
    fn resolve<'a>(
        &'a self,
        context: &'a C,
        field_name: &'a str,
        parent: Option<&'a ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>>;
}

/// Computes the ABI fingerprint for a handwritten test descriptor.
#[doc(hidden)]
pub fn descriptor_fingerprint(
    descriptor: &'static crate::SchemaDescriptor,
) -> ResolverSchemaFingerprint {
    let schema = crate::runtime_schema::build(descriptor)
        .expect("trusted static descriptor must convert to a semantic schema");
    chigiri_core::executor::generated_resolver_abi_fingerprint(&schema)
}

/// Removes the next argument, treating absence as undefined.
pub fn next_argument(values: &mut std::vec::IntoIter<CoercedInput>) -> CoercedInput {
    values.next().unwrap_or(CoercedInput::Undefined)
}

/// Verifies that no arguments remain beyond those declared by the schema field.
pub fn finish_arguments(values: &mut std::vec::IntoIter<CoercedInput>) -> FieldResult<()> {
    if values.next().is_none() {
        Ok(())
    } else {
        Err(invariant_error(
            "resolver received more arguments than the schema field defines",
        ))
    }
}

/// Requires and downcasts the parent of a nested object resolver.
pub fn required_parent<'a, T: 'static>(
    parent: Option<&'a ResolvedObject>,
    expected_type: &str,
) -> FieldResult<&'a T> {
    let parent = parent.ok_or_else(|| invariant_error("nested resolver is missing its parent"))?;
    if parent.type_name() != expected_type {
        return Err(invariant_error("resolver parent type name does not match"));
    }
    parent
        .downcast_ref()
        .ok_or_else(|| invariant_error("resolver parent application type does not match"))
}

/// Validates and downcasts an optional parent for root or nested object resolution.
pub fn optional_parent<'a, T: 'static>(
    parent: Option<&'a ResolvedObject>,
    expected_type: &str,
) -> FieldResult<Option<&'a T>> {
    parent
        .map(|parent| required_parent(Some(parent), expected_type))
        .transpose()
}

/// Converts generated-code and descriptor inconsistencies into internal errors.
pub fn invariant_error(message: impl Into<String>) -> FieldError {
    FieldError::internal(GeneratedInvariantError(message.into()))
}

pub(crate) fn is_generated_invariant(error: &FieldError) -> bool {
    error
        .internal_source()
        .is_some_and(|source| source.is::<GeneratedInvariantError>())
}

/// Converts a custom-scalar parse error to a client-safe non-field error.
pub fn scalar_input_error(error: ScalarCoercionError) -> FieldError {
    FieldError::public(error.message().to_owned()).code("BAD_USER_INPUT")
}

/// Convert enum binding error to the internal field error that holds the source.
pub fn enum_binding_error(error: crate::EnumBindingError) -> FieldError {
    FieldError::internal(error)
}

/// Validates and converts the presence invariant of required input.
pub fn required_input<T>(
    input: CoercedInput,
    convert: impl FnOnce(Value) -> FieldResult<T>,
) -> FieldResult<T> {
    match input {
        CoercedInput::Value(value) => convert(value),
        CoercedInput::Null | CoercedInput::Undefined => {
            Err(invariant_error("required input must contain a value"))
        }
    }
}

/// Converts nullable input while preserving omitted, null, and value states.
pub fn optional_input<T>(
    input: CoercedInput,
    convert: impl FnOnce(Value) -> FieldResult<T>,
) -> FieldResult<Input<T>> {
    match input {
        CoercedInput::Undefined => Ok(Input::Undefined),
        CoercedInput::Null => Ok(Input::Null),
        CoercedInput::Value(value) => convert(value).map(Input::Value),
    }
}

/// Converts nullable input with a default to `Option`.
pub fn defaulted_input<T>(
    input: CoercedInput,
    convert: impl FnOnce(Value) -> FieldResult<T>,
) -> FieldResult<Option<T>> {
    match input {
        CoercedInput::Null => Ok(None),
        CoercedInput::Value(value) => convert(value).map(Some),
        CoercedInput::Undefined => Err(invariant_error(
            "nullable input must not remain undefined after applying its default",
        )),
    }
}

/// Converts a nullable list item to `Option`.
pub fn nullable_input<T>(
    value: Value,
    convert: impl FnOnce(Value) -> FieldResult<T>,
) -> FieldResult<Option<T>> {
    match value {
        Value::Null => Ok(None),
        value => convert(value).map(Some),
    }
}

/// Converts a list value to a source-ordered `Vec`.
pub fn input_list<T>(
    value: Value,
    mut convert: impl FnMut(Value) -> FieldResult<T>,
) -> FieldResult<Vec<T>> {
    let Value::List(values) = value else {
        return Err(invariant_error(
            "coerced list value has the wrong runtime kind",
        ));
    };
    values.into_iter().map(&mut convert).collect()
}

/// Converts a coerced GraphQL `Int` to `i32`.
pub fn input_int(value: Value) -> FieldResult<i32> {
    let Value::Int(value) = value else {
        return Err(invariant_error(
            "coerced Int value has the wrong runtime kind",
        ));
    };
    i32::try_from(value).map_err(|_| invariant_error("coerced Int value is outside the i32 range"))
}

/// Convert a coerced GraphQL `Float` or `Int` to `f64`.
pub fn input_float(value: Value) -> FieldResult<f64> {
    match value {
        Value::Float(value) => Ok(value),
        Value::Int(value) => Ok(value as f64),
        _ => Err(invariant_error(
            "coerced Float value has the wrong runtime kind",
        )),
    }
}

/// Converts a coerced GraphQL `String` to an owned value.
pub fn input_string(value: Value) -> FieldResult<String> {
    let Value::String(value) = value else {
        return Err(invariant_error(
            "coerced String value has the wrong runtime kind",
        ));
    };
    Ok(value)
}

/// Converts a coerced GraphQL `Boolean` to `bool`.
pub fn input_boolean(value: Value) -> FieldResult<bool> {
    let Value::Boolean(value) = value else {
        return Err(invariant_error(
            "coerced Boolean value has the wrong runtime kind",
        ));
    };
    Ok(value)
}

/// Converts a coerced GraphQL `ID` to its canonical string newtype.
pub fn input_id(value: Value) -> FieldResult<Id> {
    input_string(value).map(Id::new)
}

/// Type-erases nullable output.
pub fn output_nullable<T>(
    value: Option<T>,
    convert: impl FnOnce(T) -> FieldResult<ResolvedValue>,
) -> FieldResult<ResolvedValue> {
    value.map_or(Ok(ResolvedValue::Null), convert)
}

/// Type-erases list output item by item.
pub fn output_list<T>(
    values: Vec<T>,
    mut convert: impl FnMut(T) -> FieldResult<ResolvedValue>,
) -> FieldResult<ResolvedValue> {
    Ok(ResolvedValue::List(
        values.into_iter().map(&mut convert).collect(),
    ))
}

/// Converts built-in scalar output to a leaf value.
pub fn output_leaf(value: impl Into<Value>) -> FieldResult<ResolvedValue> {
    Ok(ResolvedValue::Leaf(value.into()))
}

/// Validates finite built-in `Float` output and type-erases it.
pub fn output_float(value: f64) -> FieldResult<ResolvedValue> {
    if value.is_finite() {
        output_leaf(value)
    } else {
        Err(invariant_error("Float output must be finite"))
    }
}

#[derive(Debug)]
struct GeneratedInvariantError(String);

impl fmt::Display for GeneratedInvariantError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for GeneratedInvariantError {}
