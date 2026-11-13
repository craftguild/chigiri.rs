//! Closed numeric evidence; no application-owned text or error can enter it.

use std::fmt;
use std::time::Duration;

use super::ConfigurationSlot;
use crate::configuration::ConfigurationOrigin;

/// A numeric subscription setting with its unit encoded in the variant.
///
/// Durations are rendered as exact integer nanoseconds, bytes as exact integer
/// byte counts. These local construction values never become wire diagnostics.
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum ConfigurationValue {
    /// A count of subscriptions, operations, events, connections, or setups.
    Count(usize),
    /// A complete encoded-buffer or retained-metadata size in bytes.
    Bytes(usize),
    /// A finite time interval.
    Duration(Duration),
}

impl fmt::Display for ConfigurationValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Count(value) => write!(formatter, "{value} count"),
            Self::Bytes(value) => write!(formatter, "{value} bytes"),
            Self::Duration(value) => write!(formatter, "{} ns", value.as_nanos()),
        }
    }
}

impl fmt::Debug for ConfigurationValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

/// One setting participating in a fit constraint or checked multiplication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigurationOperand {
    slot: ConfigurationSlot,
    value: ConfigurationValue,
    origin: ConfigurationOrigin,
}

impl ConfigurationOperand {
    pub(crate) const fn new(
        slot: ConfigurationSlot,
        value: ConfigurationValue,
        origin: ConfigurationOrigin,
    ) -> Self {
        Self {
            slot,
            value,
            origin,
        }
    }

    /// Returns the participating setting's identity.
    pub const fn slot(self) -> ConfigurationSlot {
        self.slot
    }

    /// Returns its exact numeric value and unit.
    pub const fn value(self) -> ConfigurationValue {
        self.value
    }

    /// Returns whether its value came from the baseline or application.
    pub const fn origin(self) -> ConfigurationOrigin {
        self.origin
    }
}

/// Allocation-free evidence for an allowlisted numeric subscription constraint.
///
/// Only delivery/protocol numeric settings may attach this evidence to a
/// [`super::ConfigurationDiagnostic`]. Other configuration slots stay redacted.
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum ConfigurationConstraint {
    /// The supplied setting is outside this inclusive, same-unit interval.
    Range {
        /// Exact supplied value, including its unit.
        supplied: ConfigurationValue,
        /// Inclusive permitted minimum, in the same unit.
        minimum: ConfigurationValue,
        /// Inclusive permitted maximum, in the same unit.
        maximum: ConfigurationValue,
    },
    /// A complete encoded event cannot fit an individually valid capacity.
    Fit {
        /// Supplied encoded-event size, in bytes.
        supplied: usize,
        /// Conflicting capacity, including its value, unit, slot, and origin.
        capacity: ConfigurationOperand,
    },
    /// Multiplying these operands cannot be represented as a `usize`.
    ///
    /// The product is neither computed nor included in diagnostics.
    MultiplicationOverflow {
        /// The queued-event count operand.
        left: ConfigurationOperand,
        /// The maximum encoded-event byte operand.
        right: ConfigurationOperand,
    },
}

impl fmt::Debug for ConfigurationConstraint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for ConfigurationConstraint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Range {
                supplied,
                minimum,
                maximum,
            } => {
                write!(
                    formatter,
                    "Range {{ supplied: {supplied}, minimum: {minimum}, maximum: {maximum} }}"
                )
            }
            Self::Fit { supplied, capacity } => {
                write!(
                    formatter,
                    "Fit {{ supplied: {supplied} bytes, capacity: {capacity:?} }}"
                )
            }
            Self::MultiplicationOverflow { left, right } => {
                write!(
                    formatter,
                    "MultiplicationOverflow {{ left: {left:?}, right: {right:?} }}"
                )
            }
        }
    }
}
