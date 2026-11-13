//! Share an absolute validation deadline and sticky cancellation across submitted work.

use std::time::{Duration, Instant};

use crate::cancellation::Cancellation;

/// Shared absolute deadline and sticky cancellation passed through type checking and module initialization.
#[derive(Clone)]
pub struct ValidationControl {
    /// Absolute deadline that individual calls may only tighten.
    pub deadline: Instant,
    /// Cancellation signal shared with the validation owner.
    pub cancellation: Cancellation,
}

impl ValidationControl {
    /// Apply each budget cap while honoring the earliest remaining deadline.
    pub(crate) fn limit(&self, duration: Duration) -> Instant {
        self.deadline.min(Instant::now() + duration)
    }

    /// Fixed stop-points that do not contain values or sources.
    pub(crate) fn stopped(&self) -> Option<&'static str> {
        if self.cancellation.cancelled() {
            Some("cancelled")
        } else if Instant::now() >= self.deadline {
            Some("timeout")
        } else {
            None
        }
    }

    pub(crate) fn script(&self) -> Result<(), crate::luau::ScriptError> {
        match self.stopped() {
            Some(phase) => Err(crate::luau::ScriptError::new(phase, "validation stopped")),
            None => Ok(()),
        }
    }

    pub(crate) fn project(&self) -> Result<(), crate::project::ProjectError> {
        match self.stopped() {
            Some(phase) => {
                let mut error = crate::project::ProjectError::new(phase, "validation stopped");
                error.exit = if phase == "cancelled" { 130 } else { 2 };
                Err(error)
            }
            None => Ok(()),
        }
    }
}
