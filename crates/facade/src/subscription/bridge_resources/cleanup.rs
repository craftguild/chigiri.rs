//! Detached terminal cleanup without EndpointState or logical operation handles.

mod owner;
pub(crate) use owner::TerminalCleanupOwner;

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use super::super::delivery::{
    CancellationFlag, CleanupPhysicalCharge, CleanupTicket, EmissionAuthority, OwnerGeneration,
    QuarantineRegistry, RevokedConnectionOwner,
};
use super::admission::BridgeGeneration;
use super::error::BridgeCleanupError;

/// Cloneable cancellation observation with no endpoint or registry ownership.
#[derive(Clone)]
pub struct BridgeCancellation(CancellationFlag);

impl BridgeCancellation {
    pub(super) fn new(flag: CancellationFlag) -> Self {
        Self(flag)
    }

    /// Reports whether logical terminal handling has signalled cancellation.
    pub fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}

/// Cloneable exact-generation emission check with no writer ownership.
#[derive(Clone)]
pub struct BridgeEmissionLease {
    authority: EmissionAuthority,
    generation: OwnerGeneration,
}

impl BridgeEmissionLease {
    pub(super) fn new(authority: EmissionAuthority, generation: OwnerGeneration) -> Self {
        Self {
            authority,
            generation,
        }
    }

    /// Returns the exact endpoint-issued generation checked by this lease.
    pub fn generation(&self) -> BridgeGeneration {
        BridgeGeneration(self.generation)
    }

    /// Allows emission only while this exact generation remains authoritative.
    pub fn can_emit(&self) -> bool {
        self.authority.can_emit(self.generation)
    }
}

/// Physically charged owner after logical terminal revocation.
pub struct TerminalBridgeOperation {
    generation: OwnerGeneration,
    owner: Option<TerminalCleanupOwner>,
}

impl TerminalBridgeOperation {
    pub(super) fn new(
        generation: OwnerGeneration,
        physical: CleanupPhysicalCharge,
        ticket: CleanupTicket,
    ) -> Self {
        Self {
            generation,
            owner: Some(TerminalCleanupOwner::new(ticket, physical)),
        }
    }

    /// Returns the physically charged operation generation.
    pub fn generation(&self) -> BridgeGeneration {
        BridgeGeneration(self.generation)
    }

    /// Moves detached cleanup and every physical charge into quarantine.
    #[cfg(test)]
    pub(crate) fn quarantine(
        self,
        cleanup: DetachedCleanup,
    ) -> Result<(), Box<BridgeQuarantineFailure>> {
        let mut terminal = self;
        terminal
            .owner
            .take()
            .expect("live terminal operation")
            .quarantine(cleanup);
        Ok(())
    }

    pub(super) fn into_cleanup_owner(mut self) -> TerminalCleanupOwner {
        self.owner.take().expect("live terminal operation")
    }
}

/// Physically charged WebSocket connection after logical closure.
pub struct TerminalBridgeConnection {
    generation: OwnerGeneration,
    owner: Option<TerminalCleanupOwner>,
}

impl TerminalBridgeConnection {
    #[cfg(feature = "subscription-worker")]
    pub(super) fn bind_socket_scope(
        &mut self,
        scope: Option<std::sync::Weak<crate::managed::SocketScope>>,
    ) {
        self.owner
            .as_mut()
            .expect("live terminal connection")
            .socket_scope = scope;
    }
    /// Transfers the original connection directly to bounded recovery without
    /// constructing another active-cleanup deadline after endpoint stop.
    #[cfg(feature = "subscription-worker")]
    pub(crate) fn quarantine(self, cleanup: DetachedCleanup) {
        self.into_cleanup_owner().quarantine(cleanup);
    }

    pub(super) fn new(
        generation: OwnerGeneration,
        physical: RevokedConnectionOwner,
        ticket: CleanupTicket,
    ) -> Self {
        Self {
            generation,
            owner: Some(TerminalCleanupOwner::new(
                ticket,
                CleanupPhysicalCharge::ConnectionOwner { _permit: physical },
            )),
        }
    }

    /// Returns the endpoint-issued connection generation.
    pub fn generation(&self) -> BridgeGeneration {
        BridgeGeneration(self.generation)
    }

    pub(super) fn into_cleanup_owner(mut self) -> TerminalCleanupOwner {
        self.owner.take().expect("live terminal connection")
    }
}

/// Resource-free cleanup work created only by the facade execution owner.
pub struct DetachedCleanup {
    kind: Option<DetachedCleanupKind>,
}

enum DetachedCleanupKind {
    #[cfg(feature = "subscription-worker")]
    Completed,
    Inline(Pin<Box<dyn Future<Output = ()> + Send + 'static>>),
    #[cfg(feature = "subscription-worker")]
    Worker {
        completion: crate::subscription::worker::ErasedWorkerCompletion,
        retained: Box<crate::subscription::worker::WorkerRetainedState>,
    },
}

impl DetachedCleanup {
    /// Closed SDK metadata disposal has already completed outside all locks.
    #[cfg(feature = "subscription-worker")]
    pub(crate) fn completed() -> Self {
        Self {
            kind: Some(DetachedCleanupKind::Completed),
        }
    }

    /// The caller must capture only resource-free subscription execution state.
    pub(crate) fn new<F>(future: F) -> Self
    where
        F: Future<Output = ()> + Send + 'static,
    {
        Self {
            kind: Some(DetachedCleanupKind::Inline(Box::pin(future))),
        }
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn worker(
        completion: crate::subscription::worker::ErasedWorkerCompletion,
        retained: crate::subscription::worker::WorkerRetainedState,
    ) -> Self {
        Self {
            kind: Some(DetachedCleanupKind::Worker {
                completion,
                retained: Box::new(retained),
            }),
        }
    }

    fn into_kind(mut self) -> DetachedCleanupKind {
        self.kind.take().expect("live detached cleanup")
    }
}

impl Future for DetachedCleanup {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        match self.kind.as_mut().expect("live detached cleanup") {
            DetachedCleanupKind::Inline(future) => future.as_mut().poll(context),
            #[cfg(feature = "subscription-worker")]
            DetachedCleanupKind::Completed => Poll::Ready(()),
            #[cfg(feature = "subscription-worker")]
            DetachedCleanupKind::Worker { completion, .. } => {
                match Pin::new(completion).poll(context) {
                    Poll::Ready(
                        crate::subscription::worker::WorkerCompletionOutcome::Completed,
                    ) => Poll::Ready(()),
                    Poll::Ready(crate::subscription::worker::WorkerCompletionOutcome::Poisoned)
                    | Poll::Pending => Poll::Pending,
                }
            }
        }
    }
}

impl Drop for DetachedCleanup {
    fn drop(&mut self) {
        let Some(kind) = self.kind.take() else {
            return;
        };
        match kind {
            DetachedCleanupKind::Inline(future) => drop(future),
            #[cfg(feature = "subscription-worker")]
            DetachedCleanupKind::Completed => {}
            #[cfg(feature = "subscription-worker")]
            DetachedCleanupKind::Worker {
                completion,
                retained,
            } => {
                if completion.is_live() {
                    completion.abort();
                    completion.retire(crate::subscription::worker::RetiredWorkerOwner::orphan(
                        *retained,
                    ));
                } else {
                    drop(completion);
                    drop(retained);
                }
            }
        }
    }
}

/// Failed insertion returns both move-only owners without releasing either.
pub struct BridgeQuarantineFailure {
    error: BridgeCleanupError,
    terminal: TerminalBridgeOperation,
    cleanup: DetachedCleanup,
}

impl fmt::Debug for BridgeQuarantineFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BridgeQuarantineFailure")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

impl BridgeQuarantineFailure {
    /// Returns the closed reason without exposing cleanup or application data.
    pub fn error(&self) -> BridgeCleanupError {
        self.error
    }

    /// Returns both move-only owners so the caller can finish or release them.
    pub fn into_parts(self) -> (TerminalBridgeOperation, DetachedCleanup) {
        (self.terminal, self.cleanup)
    }
}

/// Poll-only endpoint cleanup access; it cannot insert arbitrary work.
#[derive(Clone)]
pub struct BridgeCleanupDriver {
    registry: QuarantineRegistry,
}

impl BridgeCleanupDriver {
    pub(super) fn new(registry: QuarantineRegistry) -> Self {
        Self { registry }
    }

    /// Polls at most one quarantined generation outside the registry lock.
    pub fn poll_one(&self, context: &mut Context<'_>) -> Poll<Result<bool, BridgeCleanupError>> {
        match self.registry.poll_one(context) {
            Poll::Ready(Ok(completed)) => Poll::Ready(Ok(completed.is_some())),
            Poll::Ready(Err(error)) => Poll::Ready(Err(error.into())),
            Poll::Pending => Poll::Pending,
        }
    }

    /// Returns the bounded number of pending and currently polled generations.
    pub fn pending(&self) -> Result<usize, BridgeCleanupError> {
        self.registry.len().map_err(Into::into)
    }
}
