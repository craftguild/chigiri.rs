//! Closed host failure classes that never contain a physical path or OS error.

/// A stable failure class for portable host operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum HostError {
    /// A required entry does not exist.
    Missing,
    /// An entry already occupies an exclusively created destination.
    Exists,
    /// Another owner currently holds the requested non-blocking lock.
    Busy,
    /// An observed entry or identity changed during the operation.
    Changed,
    /// A path, entry kind, or other host input is invalid.
    Path,
    /// A fixed resource limit was exceeded.
    Limit,
    /// The host cannot preserve a required security guarantee.
    Unsupported,
    /// Cancellation was requested.
    Cancelled,
    /// A host I/O operation failed without a more specific safe classification.
    Io,
}

impl std::fmt::Display for HostError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Missing => "A required file or directory is missing.",
            Self::Exists => "The destination already exists.",
            Self::Busy => "Another process is using the same host resource.",
            Self::Changed => "The path or file content was changed during the operation.",
            Self::Path => "The path, symlink, or file type is invalid.",
            Self::Limit => "The filesystem resource limit was exceeded.",
            Self::Unsupported => {
                "This host does not support the required secure filesystem operations."
            }
            Self::Cancelled => "The operation was cancelled.",
            Self::Io => "The filesystem operation failed.",
        })
    }
}

impl std::error::Error for HostError {}
