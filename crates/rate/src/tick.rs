//! The monotonic tick value is only compared within the provider epoch.

use std::fmt;

/// Monotonic tick value; this is not a wall-clock timestamp.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct MonotonicTick(u64);

impl fmt::Debug for MonotonicTick {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MonotonicTick(..)")
    }
}

impl MonotonicTick {
    pub const fn from_nanos(value: u64) -> Self {
        Self(value)
    }
    pub const fn as_nanos(self) -> u64 {
        self.0
    }
    pub const fn saturating_add(self, value: u64) -> Self {
        Self(self.0.saturating_add(value))
    }
    pub const fn saturating_sub(self, other: Self) -> u64 {
        self.0.saturating_sub(other.0)
    }
}
