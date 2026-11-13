//! Safe construction failures with no runtime or application error text.

use std::fmt;

/// An official managed service could not activate its SDK supervision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedServiceError {
    /// Construction requires an already running supported Tokio runtime.
    MissingRuntime,
    /// The endpoint is already bound to a different runtime.
    RuntimeMismatch,
    /// Runtime timer or process supervision startup failed.
    Startup,
    /// The endpoint's managed admission has already closed.
    Closed,
    /// Bounded SDK supervision storage could not be allocated.
    Capacity,
    /// Exact shared ownership could not be established.
    Infrastructure,
}

impl fmt::Display for ManagedServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MissingRuntime => "managed GraphQL service requires a running Tokio runtime",
            Self::RuntimeMismatch => "managed GraphQL endpoint is bound to another runtime",
            Self::Startup => "managed GraphQL supervision could not start",
            Self::Closed => "managed GraphQL admission is closed",
            Self::Capacity => "managed GraphQL supervision capacity is unavailable",
            Self::Infrastructure => "managed GraphQL ownership could not be established",
        })
    }
}

impl std::error::Error for ManagedServiceError {}
