//! Operation cost checks delegated to the rate provider.

use std::fmt;

/// A nonzero operation cost charged atomically before resolver execution.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct OperationCost(u64);

impl fmt::Debug for OperationCost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OperationCost(..)")
    }
}

impl OperationCost {
    /// Accepts only nonzero costs.
    pub const fn new(value: u64) -> Result<Self, OperationCostError> {
        if value == 0 {
            Err(OperationCostError::Zero)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns a unit cost.
    pub const fn one() -> Self {
        Self(1)
    }

    /// Returns the integer sent to the provider.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Adds two costs, returning an error instead of saturating on overflow.
    pub const fn checked_add(self, other: Self) -> Result<Self, OperationCostError> {
        match self.0.checked_add(other.0) {
            Some(value) => Ok(Self(value)),
            None => Err(OperationCostError::Overflow),
        }
    }

    /// Multiplies the cost, rejecting zero and arithmetic overflow.
    pub const fn checked_mul(self, multiplier: u64) -> Result<Self, OperationCostError> {
        if multiplier == 0 {
            return Err(OperationCostError::Zero);
        }
        match self.0.checked_mul(multiplier) {
            Some(value) => Ok(Self(value)),
            None => Err(OperationCostError::Overflow),
        }
    }
}

/// Error from checked cost construction or arithmetic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationCostError {
    Zero,
    Overflow,
    InvalidStructure,
    InvalidMultiplier,
    ListLimitExceeded,
}

impl fmt::Display for OperationCostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Zero => "operation cost must be at least one",
            Self::Overflow => "operation cost arithmetic overflowed",
            Self::InvalidStructure => "operation cost field structure is invalid",
            Self::InvalidMultiplier => "operation cost list multiplier is invalid",
            Self::ListLimitExceeded => "operation cost list multiplier exceeds its limit",
        })
    }
}

impl std::error::Error for OperationCostError {}
