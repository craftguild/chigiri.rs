//! Backend-neutral owned context and synchronous activation for SDK execution.

mod context;
mod future;
mod scope;

pub use context::{PropagationContext, TraceParent};
pub use future::{InstrumentedFuture, instrument_future};
pub use scope::{ExecutionScope, InstrumentationGuard, InstrumentationKind, InstrumentationScope};

#[cfg(test)]
mod tests;
