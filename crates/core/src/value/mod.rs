//! Owned value model shared by JSON transport and the GraphQL runtime.

mod id;
mod object;
mod traversal;

pub use id::Id;
pub use object::{IntoIter as ObjectIntoIter, Iter as ObjectIter, Object};
pub(crate) use traversal::{drop_iterative, within_depth_limit};

/// GraphQL value that preserves source order for objects and lists.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// GraphQL null.
    Null,
    /// Boolean value.
    Boolean(bool),
    /// `i128` is the integer value of the range JSON/GraphQL.
    Int(i128),
    /// Canonical decimal lexeme of JSON/GraphQL integers greater than `i128`.
    BigInt(String),
    /// Floating-point value. Finiteness is checked during output coercion.
    Float(f64),
    /// String value.
    String(String),
    /// Returns the list value, if this is a list.
    List(Vec<Value>),
    /// The object value to keep the insert order.
    Object(Object),
}

impl Value {
    /// Returns the object by reference, if this is an object.
    pub const fn as_object(&self) -> Option<&Object> {
        match self {
            Self::Object(object) => Some(object),
            _ => None,
        }
    }

    /// Return the list by reference, if this value is a list.
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Self::List(values) => Some(values),
            _ => None,
        }
    }

    /// Returns the string value when this is a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// Returns whether this value is null.
    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

impl From<i32> for Value {
    fn from(value: i32) -> Self {
        Self::Int(i128::from(value))
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Self::Int(i128::from(value))
    }
}

impl From<u64> for Value {
    fn from(value: u64) -> Self {
        Self::Int(i128::from(value))
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<Vec<Value>> for Value {
    fn from(value: Vec<Value>) -> Self {
        Self::List(value)
    }
}

impl From<Object> for Value {
    fn from(value: Object) -> Self {
        Self::Object(value)
    }
}

impl From<Id> for Value {
    fn from(value: Id) -> Self {
        Self::String(value.into_string())
    }
}

#[cfg(test)]
mod tests;
