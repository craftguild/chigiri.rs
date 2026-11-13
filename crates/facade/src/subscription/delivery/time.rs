//! Checked monotonic time values for delivery-owned phase deadlines.

use std::time::Duration;

/// A monotonic instant supplied by the bridge-owned clock.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct MonotonicInstant(u64);

impl MonotonicInstant {
    pub(crate) const fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }

    pub(crate) const fn as_nanos(self) -> u64 {
        self.0
    }

    pub(crate) fn checked_add(self, duration: Duration) -> Result<Self, TimerError> {
        let nanos = u64::try_from(duration.as_nanos()).map_err(|_| TimerError)?;
        self.0.checked_add(nanos).map(Self).ok_or(TimerError)
    }
}

/// A checked monotonic deadline that expires inclusively.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Deadline {
    expires_at: MonotonicInstant,
}

impl Deadline {
    pub(crate) fn after(
        started_at: MonotonicInstant,
        duration: Duration,
    ) -> Result<Self, TimerError> {
        Ok(Self {
            expires_at: started_at.checked_add(duration)?,
        })
    }

    pub(crate) const fn expires_at(self) -> MonotonicInstant {
        self.expires_at
    }

    pub(crate) const fn is_expired(self, now: MonotonicInstant) -> bool {
        now.0 >= self.expires_at.0
    }
}

/// Checked monotonic arithmetic failed and must terminate fail-closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TimerError;

/// Finite waiting after an actual shared-byte reservation failure.
///
/// Idle sources do not own this timer. Only reservation success clears it, so
/// heartbeats and unrelated work cannot extend shared-pressure retention.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct SharedPressureWait {
    deadline: Option<Deadline>,
}

impl SharedPressureWait {
    pub(crate) const fn new() -> Self {
        Self { deadline: None }
    }

    pub(crate) fn note_failure(
        &mut self,
        now: MonotonicInstant,
        finite_wait: Duration,
    ) -> Result<(), TimerError> {
        if self.deadline.is_none() {
            self.deadline = Some(Deadline::after(now, finite_wait)?);
        }
        Ok(())
    }

    pub(crate) fn note_success(&mut self) {
        self.deadline = None;
    }

    pub(crate) const fn is_armed(self) -> bool {
        self.deadline.is_some()
    }

    pub(crate) fn is_expired(self, now: MonotonicInstant) -> bool {
        self.deadline
            .is_some_and(|deadline| deadline.is_expired(now))
    }

    pub(crate) const fn deadline(self) -> Option<Deadline> {
        self.deadline
    }
}
