//! Codegen cancellation policy over the shared process owner.

#[cfg(test)]
mod cancel_tests;
#[cfg(test)]
mod tests;

pub use chigiri_host::process::{ProcessFailure, ProcessLimits, ProcessOutput};

/// Run a child without a shell and terminate any remaining descendants in its process group.
pub(crate) fn run(
    command: &mut std::process::Command,
    input: &[u8],
    limits: ProcessLimits,
    cancelled: impl Fn() -> bool,
) -> Result<ProcessOutput, ProcessFailure> {
    chigiri_host::process::run(command, input, limits, cancelled)
}
