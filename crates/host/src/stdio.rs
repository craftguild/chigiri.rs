//! Safe acquisition of inherited output handles for isolated writer children.

use std::process::Stdio;

use crate::HostError;

/// Duplicates the current process stderr for use as a child's stdout sink.
pub fn inherited_stderr() -> Result<Stdio, HostError> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::fd::AsFd;

        std::io::stderr()
            .as_fd()
            .try_clone_to_owned()
            .map(Stdio::from)
            .map_err(|_| HostError::Io)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Err(HostError::Unsupported)
    }
}
