//! Inclusive typed-source limits, distinct from encoded GraphQL response limits.

use serde::{Deserialize, Serialize};

use super::EventSettingsError;

/// Validated retained-source settings from ADR-0057 Section 6.
///
/// Waiting event count excludes the executing input. Byte charges include
/// queued and executing ownership until physical release. These bounds do not
/// describe allocator overhead, store/VM working memory or process RSS.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(try_from = "RawLimits", into = "RawLimits")]
pub struct SourceLimits {
    queued_events: usize,
    queued_bytes: usize,
    queue_age_ms: u64,
    retained_bytes: usize,
    sequence_bytes: usize,
}

impl SourceLimits {
    /// Default number of wholly waiting events per source.
    pub const DEFAULT_QUEUED_EVENTS: usize = 32;
    /// Inclusive hard maximum number of wholly waiting events per source.
    pub const MAX_QUEUED_EVENTS: usize = 1_024;
    /// Default queued/executing input byte ceiling per source.
    pub const DEFAULT_QUEUED_BYTES: usize = 256 * 1_024;
    /// Inclusive hard maximum queued/executing byte ceiling per source.
    pub const MAX_QUEUED_BYTES: usize = 4 * 1_024 * 1_024;
    /// Default maximum queue residence in monotonic milliseconds.
    pub const DEFAULT_QUEUE_AGE_MS: u64 = 10_000;
    /// Inclusive hard maximum queue residence in monotonic milliseconds.
    pub const MAX_QUEUE_AGE_MS: u64 = 60_000;
    /// Default shared retained-source byte ceiling across both consumer kinds.
    pub const DEFAULT_RETAINED_BYTES: usize = 16 * 1_024 * 1_024;
    /// Inclusive hard maximum shared retained-source byte ceiling.
    pub const MAX_RETAINED_BYTES: usize = 64 * 1_024 * 1_024;
    /// Default captured definition byte ceiling per sequence source.
    pub const DEFAULT_SEQUENCE_BYTES: usize = 1_024 * 1_024;
    /// Inclusive hard maximum captured definition byte ceiling per sequence.
    pub const MAX_SEQUENCE_BYTES: usize = 16 * 1_024 * 1_024;

    /// Validate all ranges and the cross-limit invariant without clamping.
    pub fn new(
        max_queued_events: usize,
        max_queued_bytes: usize,
        max_queue_age_ms: u64,
        max_retained_bytes: usize,
        max_sequence_bytes: usize,
    ) -> Result<Self, EventSettingsError> {
        if !(1..=Self::MAX_QUEUED_EVENTS).contains(&max_queued_events)
            || !(1..=Self::MAX_QUEUED_BYTES).contains(&max_queued_bytes)
            || !(1..=Self::MAX_QUEUE_AGE_MS).contains(&max_queue_age_ms)
            || !(1..=Self::MAX_RETAINED_BYTES).contains(&max_retained_bytes)
            || !(1..=Self::MAX_SEQUENCE_BYTES).contains(&max_sequence_bytes)
        {
            return Err(EventSettingsError::LimitRange);
        }
        if max_sequence_bytes > max_retained_bytes {
            return Err(EventSettingsError::SequenceRetention);
        }
        Ok(Self {
            queued_events: max_queued_events,
            queued_bytes: max_queued_bytes,
            queue_age_ms: max_queue_age_ms,
            retained_bytes: max_retained_bytes,
            sequence_bytes: max_sequence_bytes,
        })
    }

    /// Number of wholly waiting events allowed per source.
    pub const fn max_queued_events(self) -> usize {
        self.queued_events
    }

    /// Queued/executing retained input bytes allowed per source.
    pub const fn max_queued_bytes(self) -> usize {
        self.queued_bytes
    }

    /// Maximum monotonic residence of an unprocessed item, in milliseconds.
    pub const fn max_queue_age_ms(self) -> u64 {
        self.queue_age_ms
    }

    /// Shared physical retained-source ceiling, including sequence definitions.
    pub const fn max_retained_bytes(self) -> usize {
        self.retained_bytes
    }

    /// Captured definition byte ceiling for one sequence.
    pub const fn max_sequence_bytes(self) -> usize {
        self.sequence_bytes
    }
}

impl Default for SourceLimits {
    fn default() -> Self {
        Self {
            queued_events: Self::DEFAULT_QUEUED_EVENTS,
            queued_bytes: Self::DEFAULT_QUEUED_BYTES,
            queue_age_ms: Self::DEFAULT_QUEUE_AGE_MS,
            retained_bytes: Self::DEFAULT_RETAINED_BYTES,
            sequence_bytes: Self::DEFAULT_SEQUENCE_BYTES,
        }
    }
}

// Raw deserialization must pass the same constructor as host callers. In
// particular, omitted fields default individually without accepting bad values.
#[derive(Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct RawLimits {
    max_queued_events: usize,
    max_queued_bytes: usize,
    max_queue_age_ms: u64,
    max_retained_bytes: usize,
    max_sequence_bytes: usize,
}

impl Default for RawLimits {
    fn default() -> Self {
        SourceLimits::default().into()
    }
}

impl From<SourceLimits> for RawLimits {
    fn from(value: SourceLimits) -> Self {
        Self {
            max_queued_events: value.queued_events,
            max_queued_bytes: value.queued_bytes,
            max_queue_age_ms: value.queue_age_ms,
            max_retained_bytes: value.retained_bytes,
            max_sequence_bytes: value.sequence_bytes,
        }
    }
}

impl TryFrom<RawLimits> for SourceLimits {
    type Error = EventSettingsError;

    fn try_from(value: RawLimits) -> Result<Self, Self::Error> {
        Self::new(
            value.max_queued_events,
            value.max_queued_bytes,
            value.max_queue_age_ms,
            value.max_retained_bytes,
            value.max_sequence_bytes,
        )
    }
}
