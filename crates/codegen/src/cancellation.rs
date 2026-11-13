//! Scoped cancellation propagation for code-generation operations.

mod signals;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::CodegenError;

/// One-way cancellation token that another thread or signal handler may request.
/// Library operations do not register signal handlers automatically; they observe only the scope
/// established by [`Cancellation::run`].
#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);

struct Context {
    token: Cancellation,
    parent: Option<Rc<Context>>,
}

thread_local! {
    static CURRENT: RefCell<Option<Rc<Context>>> = const { RefCell::new(None) };
}

impl Cancellation {
    /// Requests cancellation for every scope that uses this token.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Make this token current for the duration of `operation` on the calling thread.
    /// Spawned threads can establish the same scope by cloning the token and calling `run`.
    pub fn run<R>(&self, operation: impl FnOnce() -> R) -> R {
        let parent = CURRENT.with(|current| current.borrow().clone());
        let _scope = Scope::replace(Some(Rc::new(Context {
            token: self.clone(),
            parent,
        })));
        operation()
    }
}

struct Scope(Option<Rc<Context>>);

impl Scope {
    fn replace(context: Option<Rc<Context>>) -> Self {
        Self(CURRENT.with(|current| current.replace(context)))
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|current| current.replace(self.0.take()));
    }
}

pub(crate) fn requested() -> bool {
    CURRENT.with(|current| {
        let current = current.borrow();
        let mut context = current.as_deref();
        while let Some(value) = context {
            if value.token.0.load(Ordering::SeqCst) {
                return true;
            }
            context = value.parent.as_deref();
        }
        false
    })
}

pub(crate) fn check() -> Result<(), CodegenError> {
    if requested() { Err(error()) } else { Ok(()) }
}

/// Check the current cancellation scope before the CLI writes final output.
#[doc(hidden)]
pub fn check_cancellation() -> Result<(), CodegenError> {
    check()
}

pub(crate) fn error() -> CodegenError {
    CodegenError::new("SF4026", "The operation was cancelled.")
}

pub(crate) fn filesystem() -> Result<(), crate::filesystem::FsError> {
    if requested() {
        Err(crate::filesystem::FsError::Cancelled)
    } else {
        Ok(())
    }
}

/// Run rollback or cleanup outside cancellation while preserving the pending request afterward.
pub(crate) fn recovering<R>(operation: impl FnOnce() -> R) -> R {
    let _scope = Scope::replace(None);
    operation()
}

/// Run a process under the current cancellation scope and one cleanup deadline.
#[doc(hidden)]
pub fn run_bounded_process(
    command: &mut std::process::Command,
    input: &[u8],
    limits: crate::ProcessLimits,
) -> Result<crate::ProcessOutput, crate::ProcessFailure> {
    crate::process::run(command, input, limits, requested)
}

/// Connects CLI `TERM` and `INT` signals to a synchronous operation.
///
/// This installs a process-global handler, so library callers should normally use
/// [`Cancellation`] directly. Help and version commands do not call this function.
#[doc(hidden)]
pub fn with_signal_cancellation<R>(operation: impl FnOnce() -> R) -> Result<R, CodegenError> {
    signals::run(operation)
}
