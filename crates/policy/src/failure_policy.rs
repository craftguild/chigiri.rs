//! Request-continuation policy for provider failures.

/// Request-continuation policy for provider failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderFailurePolicy {
    FailOpen,
    FailClosed,
}
