//! Resolve scalar representations, list bounds, and leaf constraints into one IR.

mod domains;
mod format;
mod parse;
mod pattern;
mod scalar_spec;

use std::sync::Arc;

pub(crate) use domains::key as validate_key;
pub use format::Format;
pub use pattern::Pattern;
pub(crate) use pattern::PatternRegistry;

/// JSON representation required for a custom scalar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Representation {
    /// UTF-8 string.
    String,
    /// Boolean value.
    Boolean,
    /// Signed integer.
    Integer,
    /// Finite floating-point number.
    Number,
    /// JSON object.
    Object,
    /// JSON array.
    Array,
    /// Bounded JSON value.
    Json,
}

/// Constraints shared by list bounds and scalar values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Constraints {
    /// Inclusive numeric lower bound.
    pub min: Option<f64>,
    /// Inclusive numeric upper bound.
    pub max: Option<f64>,
    /// Exclusive numeric lower bound.
    pub exclusive_min: Option<f64>,
    /// Exclusive numeric upper bound.
    pub exclusive_max: Option<f64>,
    /// Required finite numeric multiple.
    pub multiple_of: Option<f64>,
    /// Minimum UTF-8 byte length.
    pub min_bytes: Option<usize>,
    /// Maximum UTF-8 byte length.
    pub max_bytes: Option<usize>,
    /// Minimum number of Unicode scalar values.
    pub min_length: Option<usize>,
    /// The maximum number of Unicode scalar values.
    pub max_length: Option<usize>,
    /// Required string prefix.
    pub starts_with: Option<Arc<str>>,
    /// Required string suffix.
    pub ends_with: Option<Arc<str>>,
    /// Required substring.
    pub contains: Option<Arc<str>>,
    /// Forbidden substring.
    pub not_contains: Option<Arc<str>>,
    /// Regex compiled once while building the schema.
    pub pattern: Option<Arc<Pattern>>,
    /// One of the eight supported string formats.
    pub format: Option<Format>,
    /// Minimum item count for a list or array-represented scalar.
    pub min_items: Option<usize>,
    /// Maximum item count for a list or array-represented scalar.
    pub max_items: Option<usize>,
}

/// Scalar contract declared explicitly by directive rather than inferred from its name.
#[derive(Clone, Debug, PartialEq)]
pub struct ScalarSpec {
    /// Declared JSON representation.
    pub representation: Representation,
    /// Constraints applicable to the representation.
    pub constraints: Constraints,
}
