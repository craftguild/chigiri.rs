//! Schema policy values and build errors.

use std::fmt;

/// Controls availability of the `__schema` and `__type` introspection fields.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum IntrospectionPolicy {
    /// Rejects schema introspection, the production default.
    #[default]
    Disabled,
    /// The application explicitly allows schema introspection.
    Enabled,
}

/// Bounded capacity of a schema instance's document cache.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DocumentCacheLimits {
    /// Maximum number of validated documents.
    pub max_entries: usize,
    /// Maximum total bytes of query text used as cache keys.
    pub max_query_bytes: usize,
}

impl DocumentCacheLimits {
    /// Returns settings that disable the cache.
    pub const fn disabled() -> Self {
        Self {
            max_entries: 0,
            max_query_bytes: 0,
        }
    }
}

impl Default for DocumentCacheLimits {
    fn default() -> Self {
        Self {
            max_entries: 1_024,
            max_query_bytes: 16 * 1024 * 1024,
        }
    }
}

/// ABI or internal compatibility error in a generated schema descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaBuildError {
    message: String,
}

impl SchemaBuildError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for SchemaBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SchemaBuildError {}
