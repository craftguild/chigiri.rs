//! Host interrupts update only a shared flag; the calling thread performs I/O and rollback.

use super::Cancellation;
use crate::CodegenError;

pub(super) fn run<R>(operation: impl FnOnce() -> R) -> Result<R, CodegenError> {
    let token = Cancellation::default();
    let _registration = chigiri_host::interrupt::register(token.0.clone()).map_err(|_| {
        CodegenError::new(
            "SF4008",
            "Failed to register the TERM/INT cancellation handler.",
        )
    })?;
    Ok(token.run(operation))
}
