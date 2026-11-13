//! Long-lived child-tree ownership used by isolated output transports.

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod unsupported;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use unix::OwnedChild;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub use unsupported::OwnedChild;

/// Stable failures for long-lived child ownership.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildError {
    /// The configured executable is unavailable.
    Unavailable,
    /// The host environment could not start the child.
    Environment,
    /// The backend is not available with the required guarantees.
    Unsupported,
    /// Termination or final wait ownership failed within the deadline.
    Cleanup,
}
