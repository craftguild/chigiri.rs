//! Safe failures from the concrete subscription worker runtime.

use std::error::Error;
use std::fmt;

/// Failure to start, use, or drain the subscription worker runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub enum SubscriptionWorkerRuntimeError {
    /// The process reaper thread could not be started.
    Startup,
    /// The process runtime has entered its final one-shot shutdown.
    Shutdown,
    /// This endpoint has begun shutdown and accepts no new workers.
    EndpointClosed,
    /// Every endpoint-owned completion slot is occupied.
    Capacity,
    /// A worker or completion owner violated the sealed ownership protocol.
    Invariant,
    /// Application destruction panicked, so physical charges remain retained.
    Poisoned,
}

impl fmt::Display for SubscriptionWorkerRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Startup => "subscription worker runtime startup failed",
            Self::Shutdown => "subscription worker runtime is shut down",
            Self::EndpointClosed => "subscription endpoint worker runtime is closed",
            Self::Capacity => "subscription worker completion capacity is exhausted",
            Self::Invariant => "subscription worker ownership invariant failed",
            Self::Poisoned => "subscription worker completion is poisoned",
        })
    }
}

impl Error for SubscriptionWorkerRuntimeError {}
