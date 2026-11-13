//! Owned event configuration and source contracts, independent of any Luau VM.

mod limits;
pub(crate) mod payload;
mod registry;
pub(crate) mod retention;
#[cfg(test)]
mod tests;

pub use limits::SourceLimits;
pub(crate) use registry::validate_name;
pub use registry::{ConsumerRegistration, EventSettings, ResolvedConsumer};

/// Bounded structural or semantic configuration failure without authored values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventSettingsError {
    /// A source limit is zero or exceeds its inclusive hard maximum.
    LimitRange,
    /// A sequence definition ceiling exceeds the shared retained-source ceiling.
    SequenceRetention,
    /// The registry exceeds its fixed capacity, including disabled entries.
    ConsumerCapacity,
    /// A registration name is not a portable ASCII identifier of 1 through 64 bytes.
    ConsumerName,
    /// Two registration names collide under ASCII case folding.
    ConsumerCollision,
    /// A registered field is not an adopted Subscription root field.
    ConsumerField,
}

impl std::fmt::Display for EventSettingsError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(match self {
            Self::LimitRange => "source limit is outside its inclusive bounds",
            Self::SequenceRetention => "sequence bytes exceed shared retained bytes",
            Self::ConsumerCapacity => "consumer registration capacity exceeded",
            Self::ConsumerName => "consumer name must be a bounded ASCII identifier",
            Self::ConsumerCollision => "consumer names collide under ASCII case folding",
            Self::ConsumerField => "consumer requires an adopted Subscription root field",
        })
    }
}

impl std::error::Error for EventSettingsError {}
