//! Bounded opaque persisted-operation IDs and catalog revisions.

use std::fmt;

const ID_BYTES: usize = 256;
const REVISION_BYTES: usize = 128;

/// The application registry defines the opaque persisted operation ID.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PersistedOperationId(String);

impl PersistedOperationId {
    /// Creates an ID from 1 to 256 visible ASCII bytes.
    pub fn new(value: impl Into<String>) -> Result<Self, PersistedOperationIdError> {
        bounded_visible_ascii(value.into(), ID_BYTES).map(Self)
    }

    /// Returns the validated ID used for provider lookup.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PersistedOperationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PersistedOperationId")
            .field("bytes", &self.0.len())
            .finish_non_exhaustive()
    }
}

/// An immutable catalog revision associated with a deployment.
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct CatalogRevision(String);

impl CatalogRevision {
    /// Creates a revision from 1 to 128 visible ASCII bytes.
    pub fn new(value: impl Into<String>) -> Result<Self, PersistedOperationIdError> {
        bounded_visible_ascii(value.into(), REVISION_BYTES).map(Self)
    }

    /// Returns the revision exposed through readiness metadata.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for CatalogRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CatalogRevision")
            .field("bytes", &self.0.len())
            .finish_non_exhaustive()
    }
}

/// A persisted ID or revision failed character or byte limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PersistedOperationIdError;

impl fmt::Display for PersistedOperationIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("persisted operation identity is empty, invalid, or too long")
    }
}

impl std::error::Error for PersistedOperationIdError {}

fn bounded_visible_ascii(
    value: String,
    max_bytes: usize,
) -> Result<String, PersistedOperationIdError> {
    if value.is_empty()
        || value.len() > max_bytes
        || !value.bytes().all(|byte| matches!(byte, 0x21..=0x7e))
    {
        return Err(PersistedOperationIdError);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::PersistedOperationId;

    #[test]
    fn debug_does_not_expose_the_id() {
        let id = PersistedOperationId::new("secret-id").unwrap();
        assert!(!format!("{id:?}").contains("secret-id"));
        assert!(PersistedOperationId::new("bad id").is_err());
        assert!(PersistedOperationId::new("x".repeat(257)).is_err());
    }
}
