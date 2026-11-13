//! Validated scoped lifecycle options, independent of schema configuration.

use std::fmt;
use std::time::{Duration, Instant};

/// Safe, sticky result of one endpoint's scoped managed shutdown.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShutdownOutcome {
    /// Eligible SDK work and scoped active cleanup completed within the deadline.
    Drained,
    /// Cancellation was requested without a worker-completion guarantee.
    CancellationRequested,
    /// The original SDK deadline expired; physical owners may still be retained.
    Expired,
    /// SDK supervision or exact ownership failed; admission is closed.
    InfrastructureFailure,
}

/// Policy for stopping an endpoint's managed GraphQL work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShutdownMode {
    /// Close GraphQL admission and request cancellation without waiting for join.
    Immediate,
    /// Stop admission and source demand, then wait for eligible in-flight work.
    Graceful,
}

/// Immutable SDK shutdown policy used with a native server notification hook.
///
/// Graceful uses one maximum duration for SDK drain and active cleanup. It does
/// not bound the native server's subsequent shutdown, preempt blocking Rust
/// code, roll back committed writes, or establish client receipt. The application
/// supplies its signal and continues to own the listener, runtime and routes.
/// These options are separate from the immutable schema/component graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShutdownOptions {
    mode: ShutdownMode,
    max_duration: Option<Duration>,
}

impl ShutdownOptions {
    /// Requests cancellation immediately without awaiting worker completion.
    pub const fn immediate() -> Self {
        Self {
            mode: ShutdownMode::Immediate,
            max_duration: None,
        }
    }

    /// Selects Graceful with the 30-second maximum SDK wait.
    pub const fn graceful() -> Self {
        Self {
            mode: ShutdownMode::Graceful,
            max_duration: Some(Duration::from_secs(30)),
        }
    }

    /// Selects Graceful with a positive, finite, schedulable maximum wait.
    ///
    /// The duration starts at the accepted shutdown notification, not at option
    /// construction. Repeated notifications do not restart it. A shorter
    /// request, event or send deadline still wins.
    ///
    /// ```
    /// use chigiri::{ShutdownMode, ShutdownOptions};
    /// use std::time::Duration;
    /// let options = ShutdownOptions::graceful_for(Duration::from_secs(10))?;
    /// assert_eq!(options.mode(), ShutdownMode::Graceful);
    /// assert_eq!(options.max_duration(), Some(Duration::from_secs(10)));
    /// # Ok::<(), chigiri::ShutdownOptionsError>(())
    /// ```
    pub fn graceful_for(max_duration: Duration) -> Result<Self, ShutdownOptionsError> {
        if max_duration.is_zero() {
            return Err(ShutdownOptionsError::ZeroDuration);
        }
        if max_duration == Duration::MAX {
            return Err(ShutdownOptionsError::UnboundedDuration);
        }
        // Leave room for timer rounding as well as the monotonic deadline.
        // Checking addition avoids either a panic or an implicitly infinite wait.
        let rounded = max_duration.checked_add(Duration::from_millis(1));
        if rounded
            .and_then(|duration| Instant::now().checked_add(duration))
            .is_none()
        {
            return Err(ShutdownOptionsError::UnschedulableDuration);
        }
        Ok(Self {
            mode: ShutdownMode::Graceful,
            max_duration: Some(max_duration),
        })
    }

    /// Returns the selected cancellation/drain policy.
    pub const fn mode(self) -> ShutdownMode {
        self.mode
    }

    /// Returns the Graceful SDK wait; Immediate does not wait for completion.
    pub const fn max_duration(self) -> Option<Duration> {
        self.max_duration
    }
}

/// A shutdown duration cannot form a finite supported SDK deadline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShutdownOptionsError {
    /// A zero duration cannot express Graceful; select Immediate explicitly.
    ZeroDuration,
    /// The unbounded duration sentinel is not a shutdown deadline.
    UnboundedDuration,
    /// The duration cannot be represented by a safe monotonic timer deadline.
    UnschedulableDuration,
}

impl fmt::Display for ShutdownOptionsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ZeroDuration => "graceful shutdown duration must be positive",
            Self::UnboundedDuration => "graceful shutdown duration must be finite",
            Self::UnschedulableDuration => "graceful shutdown deadline cannot be scheduled",
        })
    }
}

impl std::error::Error for ShutdownOptionsError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_graceful_immediate_and_positive_override_are_distinct() {
        assert_eq!(ShutdownOptions::graceful().mode(), ShutdownMode::Graceful);
        assert_eq!(
            ShutdownOptions::graceful().max_duration(),
            Some(Duration::from_secs(30))
        );
        assert_eq!(ShutdownOptions::immediate().mode(), ShutdownMode::Immediate);
        assert_eq!(ShutdownOptions::immediate().max_duration(), None);
        for duration in [Duration::from_nanos(1), Duration::from_secs(10)] {
            let options = ShutdownOptions::graceful_for(duration).unwrap();
            assert_eq!(options.mode(), ShutdownMode::Graceful);
            assert_eq!(options.max_duration(), Some(duration));
        }
    }

    #[test]
    fn invalid_overrides_return_typed_errors_without_panicking() {
        assert_eq!(
            ShutdownOptions::graceful_for(Duration::ZERO),
            Err(ShutdownOptionsError::ZeroDuration)
        );
        assert_eq!(
            ShutdownOptions::graceful_for(Duration::MAX),
            Err(ShutdownOptionsError::UnboundedDuration)
        );
        assert_eq!(
            ShutdownOptions::graceful_for(Duration::from_secs(u64::MAX - 1)),
            Err(ShutdownOptionsError::UnschedulableDuration)
        );
    }
}
