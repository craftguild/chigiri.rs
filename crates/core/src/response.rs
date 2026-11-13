use std::fmt;

use crate::{Object, Value};

/// One-based position in GraphQL source text.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Location {
    /// 1-based, line number.
    pub line: usize,
    /// 1-based Unicode scalar column. Tabs are also counted as a single letter.
    pub column: usize,
}

impl Location {
    /// Creates a position from one-based line and column values.
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

/// Segment of a response error path.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum PathSegment {
    /// The field name or alias of the response object.
    Field(String),
    /// The 0-based index in the response list.
    Index(usize),
}

impl From<String> for PathSegment {
    fn from(value: String) -> Self {
        Self::Field(value)
    }
}

impl From<&str> for PathSegment {
    fn from(value: &str) -> Self {
        Self::Field(value.to_owned())
    }
}

impl From<usize> for PathSegment {
    fn from(value: usize) -> Self {
        Self::Index(value)
    }
}

/// A client-visible GraphQL error.
#[derive(Clone, PartialEq)]
pub struct GraphQlError {
    /// Client-safe message.
    pub message: String,
    /// Relevant locations in the query document.
    pub locations: Vec<Location>,
    /// Path from the response root to the error position.
    pub path: Vec<PathSegment>,
    /// Additional metadata such as machine-readable codes.
    pub extensions: Object,
}

impl fmt::Debug for GraphQlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GraphQlError")
            .field("message_bytes", &self.message.len())
            .field("location_count", &self.locations.len())
            .field("path_segments", &self.path.len())
            .field("extension_count", &self.extensions.len())
            .finish()
    }
}

impl GraphQlError {
    /// Creates an error with only a message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            locations: Vec::new(),
            path: Vec::new(),
            extensions: Object::new(),
        }
    }

    /// Sets `extensions.code`.
    pub fn code(mut self, code: impl Into<String>) -> Self {
        self.extensions.insert("code", code.into());
        self
    }

    /// Sets an extension value.
    pub fn extension(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.extensions.insert(key, value);
        self
    }
}

/// GraphQL execution or request error response.
#[derive(Clone, PartialEq)]
pub struct GraphQlResponse {
    /// `None` means execution has not started; `Some(Value::Null)` is a null
    /// execution result.
    pub data: Option<Value>,
    /// GraphQL errors in occurrence order.
    pub errors: Vec<GraphQlError>,
    /// Additional metadata for the whole response.
    pub extensions: Object,
}

impl fmt::Debug for GraphQlResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GraphQlResponse")
            .field("has_data", &self.data.is_some())
            .field("error_count", &self.errors.len())
            .field("extension_count", &self.extensions.len())
            .finish()
    }
}

impl GraphQlResponse {
    /// Create a response containing execution data.
    pub fn executed(data: Value) -> Self {
        Self {
            data: Some(data),
            errors: Vec::new(),
            extensions: Object::new(),
        }
    }

    /// Creates a request-error response before execution begins.
    pub fn request_error(error: GraphQlError) -> Self {
        Self {
            data: None,
            errors: vec![error],
            extensions: Object::new(),
        }
    }

    /// Appends an error.
    pub fn with_error(mut self, error: GraphQlError) -> Self {
        self.errors.push(error);
        self
    }
}
