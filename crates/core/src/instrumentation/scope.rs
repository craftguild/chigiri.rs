//! Owned activation and exactly-once span completion without a global registry.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{InstrumentedFuture, PropagationContext};
use crate::{ErrorEvent, LifecyclePhase, LifecycleResult, SamplingDecision};

/// Closed SDK work categories; backend naming and attributes remain opt-in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstrumentationKind {
    /// A finite HTTP or direct request.
    Request,
    /// Subscription initialization, ending before source waiting.
    SubscriptionSetup,
    /// One admitted subscription event with an independent root and setup link.
    SubscriptionEvent,
    /// Parsing, validation, selection, and operation resolution.
    OperationResolution,
    /// Actual operation execution.
    Execution,
    /// One SDK-invoked provider under the appropriate operation.
    Provider {
        /// Existing closed lifecycle phase.
        phase: LifecyclePhase,
        /// Provider's static component name.
        component: &'static str,
    },
    /// One actual field invocation when field instrumentation is enabled.
    Field,
}

/// A synchronous backend guard that restores ambient context on destruction.
///
/// Guards need not be `Send` and never cross a future poll or an await. Backend
/// implementations must restore both parent context and suppression, including
/// during unwind, and must not perform blocking I/O or panic in guard Drop.
pub trait InstrumentationGuard {}

/// Object-safe backend scope used only by SDK-owned instrumentation boundaries.
///
/// Implementations retain sanitized context and their own span, never application
/// context or credentials. Activation and completion must not block on export.
/// Ordinary telemetry failure degrades instrumentation within its bounds.
pub trait ExecutionScope: Send + Sync + 'static {
    /// Installs this owned context for one synchronous call or future poll.
    fn enter(&self) -> Box<dyn InstrumentationGuard + '_>;
    /// Returns immutable correlation/control values without a live span.
    fn context(&self) -> PropagationContext;
    /// Starts child work inside its already-active owning execution context.
    fn child(&self, kind: InstrumentationKind, sampling: SamplingDecision) -> InstrumentationScope;
    /// Records a safe error inside the already-active owning execution.
    /// Independent observers remain separate.
    fn observe_error(&self, event: &ErrorEvent<'_>);
    /// Ends this scope once; called by the shared owner after terminal election.
    fn finish(&self, result: LifecycleResult);
}

/// Shared scope lifetime, independent of request IDs and SDK physical permits.
#[derive(Clone)]
pub struct InstrumentationScope(Arc<Owner>);

struct Owner {
    scope: Box<dyn ExecutionScope>,
    finished: AtomicBool,
}

impl InstrumentationScope {
    /// Owns one backend scope and its terminal election.
    pub fn new(scope: impl ExecutionScope) -> Self {
        Self(Arc::new(Owner {
            scope: Box::new(scope),
            finished: AtomicBool::new(false),
        }))
    }

    /// Activates owned context only while the synchronous callback executes.
    pub fn run<R>(&self, callback: impl FnOnce() -> R) -> R {
        let _guard = self.0.scope.enter();
        callback()
    }

    /// Returns only the bounded values allowed for subscription retention.
    pub fn context(&self) -> PropagationContext {
        self.0.scope.context()
    }

    /// Creates a child while this scope's application execution is active.
    ///
    /// The backend captures nested owning context/suppression at this boundary.
    /// Callers do not reconstruct it from an unrelated worker's ambient state.
    pub fn child(&self, kind: InstrumentationKind, sampling: SamplingDecision) -> Self {
        self.0.scope.child(kind, sampling)
    }

    /// Converts an error inside this scope's already-active owning execution.
    ///
    /// As with `child`, callers activate the appropriate SDK boundary first.
    /// Reinstalling the initial context here would erase nested owning suppression.
    pub fn observe_error(&self, event: &ErrorEvent<'_>) {
        self.0.scope.observe_error(event);
    }

    /// Elects the first completion without retrying a backend callback.
    pub fn finish(&self, result: LifecycleResult) {
        self.0.finish(result);
    }

    /// Activates this scope for every poll and for owned future destruction.
    pub fn instrument<F: std::future::Future>(&self, future: F) -> InstrumentedFuture<F> {
        InstrumentedFuture::new(self.clone(), future)
    }
}

impl Owner {
    fn finish(&self, result: LifecycleResult) {
        if !self.finished.swap(true, Ordering::AcqRel) {
            let _guard = self.scope.enter();
            self.scope.finish(result);
        }
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        self.finish(LifecycleResult::Cancelled);
    }
}

impl fmt::Debug for InstrumentationScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InstrumentationScope")
            .finish_non_exhaustive()
    }
}
