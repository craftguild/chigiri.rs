//! Custom-scalar callbacks and built-in scalar conversion.

use std::collections::BTreeMap;
use std::fmt;

use crate::Value;

/// Client-safe custom-scalar input error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScalarCoercionError {
    message: String,
}

impl ScalarCoercionError {
    /// Creates an internal error whose source message is not client-safe.
    pub fn public(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Return the message that may be exposed to the client.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ScalarCoercionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ScalarCoercionError {}

type Callback = dyn Fn(&str, Value) -> Result<Value, ScalarCoercionError> + Send + Sync;

/// Request-independent registry mapping scalar names to type-erased input callbacks.
///
/// The callback receives the scalar name and literal/variable, which is normalised to the core `Value`.
/// Panics propagate; ordinary coercion failures are returned as [`ScalarCoercionError`].
#[derive(Default)]
pub struct CustomScalarRegistry {
    callbacks: BTreeMap<String, Box<Callback>>,
}

impl CustomScalarRegistry {
    /// Creates an empty callback registry.
    pub const fn new() -> Self {
        Self {
            callbacks: BTreeMap::new(),
        }
    }

    /// Registers a scalar callback, replacing any callback with the same name.
    pub fn register<F>(&mut self, scalar_name: impl Into<String>, callback: F)
    where
        F: Fn(&str, Value) -> Result<Value, ScalarCoercionError> + Send + Sync + 'static,
    {
        self.callbacks
            .insert(scalar_name.into(), Box::new(callback));
    }

    /// Returns the registered scalar name.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.callbacks.keys().map(String::as_str)
    }

    pub(super) fn coerce(
        &self,
        scalar_name: &str,
        value: Value,
    ) -> Result<Value, ScalarCoercionError> {
        let Some(callback) = self.callbacks.get(scalar_name) else {
            return Err(ScalarCoercionError::public(format!(
                "No input coercer is registered for custom scalar `{scalar_name}`"
            )));
        };
        callback(scalar_name, value)
    }
}
