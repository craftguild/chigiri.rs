//! Closed, credential-free failures for resource transitions.

use super::super::delivery::{PermitError, QuarantineError};

/// Admission or ownership transfer failed without partially acquiring capacity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgeResourceError {
    /// The configured endpoint or connection capacity is already charged.
    Capacity,
    /// The connection already owns this logical operation ID.
    DuplicateId,
    /// The logical operation ID is outside the bounded bridge contract.
    InvalidId,
    /// The endpoint cannot issue another unique owner generation.
    GenerationExhausted,
    /// An operation transition did not address its exact current generation.
    StaleGeneration,
    /// This transport cannot activate the requested operation kind.
    InvalidOperationKind,
    /// Internal ownership accounting could not be preserved.
    Invariant,
}

impl std::fmt::Display for BridgeResourceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("subscription resource admission failed")
    }
}

impl std::error::Error for BridgeResourceError {}

impl From<PermitError> for BridgeResourceError {
    fn from(error: PermitError) -> Self {
        match error {
            PermitError::Capacity => Self::Capacity,
            PermitError::DuplicateId => Self::DuplicateId,
            PermitError::InvalidId => Self::InvalidId,
            PermitError::GenerationExhausted => Self::GenerationExhausted,
            PermitError::StaleGeneration => Self::StaleGeneration,
            PermitError::Invariant => Self::Invariant,
        }
    }
}

/// Bounded cleanup registry operation failed closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgeCleanupError {
    /// Every possible physically charged owner already has a cleanup entry.
    Capacity,
    /// The exact logical ID and generation are already being cleaned up.
    DuplicateGeneration,
    /// Registry ownership accounting could not be preserved.
    Invariant,
}

impl From<QuarantineError> for BridgeCleanupError {
    fn from(error: QuarantineError) -> Self {
        match error {
            QuarantineError::Capacity => Self::Capacity,
            QuarantineError::DuplicateGeneration => Self::DuplicateGeneration,
            QuarantineError::Invariant => Self::Invariant,
        }
    }
}
