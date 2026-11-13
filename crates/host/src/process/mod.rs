//! Bounded child execution and process-tree lifetime ownership.

mod owned;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;

pub use owned::{ChildError, OwnedChild};

use std::process::{Command, ExitStatus};
use std::time::Duration;

/// The fixed limit for one process tree.
#[derive(Clone, Copy, Debug)]
pub struct ProcessLimits {
    /// The deadline includes spawn, pipe processing, termination, and reaping.
    pub timeout: Duration,
    /// Limits stdin bytes and the combined bytes of stdout and stderr.
    pub bytes: usize,
}

/// Captured output after the complete process tree is recovered.
#[derive(Debug)]
pub struct ProcessOutput {
    /// The final status of the process-group leader.
    pub status: ExitStatus,
    /// Captured stdout bytes.
    pub stdout: Vec<u8>,
    /// Captured stderr bytes.
    pub stderr: Vec<u8>,
}

/// Stable process failure classes without raw OS errors or command values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessFailure {
    /// The host lacks a reviewed process-tree backend.
    UnsupportedHost,
    /// The child or its pipes could not be created.
    Start,
    /// The process reached its deadline.
    Timeout,
    /// Cancellation was requested before spawning or during recovery.
    Cancelled,
    /// Input or combined output exceeded its byte limit.
    Limit,
    /// Pipe I/O failed.
    Io,
    /// The leader status could not be obtained.
    Wait,
    /// Termination and reaping did not finish before the deadline.
    Cleanup,
}

/// Runs a child without a shell and owns termination of its remaining tree.
pub fn run(
    command: &mut Command,
    input: &[u8],
    limits: ProcessLimits,
    cancelled: impl Fn() -> bool,
) -> Result<ProcessOutput, ProcessFailure> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        unix::run(command, input, limits, &cancelled)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (command, input, limits, cancelled);
        Err(ProcessFailure::UnsupportedHost)
    }
}
