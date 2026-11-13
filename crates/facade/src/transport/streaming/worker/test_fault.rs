//! Request-local faults exercise real post-callback commit and byte transitions.

use crate::subscription::delivery::EndpointByteOwner;
use crate::subscription::worker::InitialResponseDeadline;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};

#[derive(Clone)]
pub(crate) enum InitialCommitTestFault {
    None,
    Commit,
    ShrinkOnce,
    ShrinkAlways(Arc<AtomicUsize>),
    MissingBuffer,
    CorruptedBytes,
    PauseBeforeCommit(InitialCommitTestBarrier),
    WorkerProbe(InitialWorkerProbe),
}

impl InitialCommitTestFault {
    pub(super) fn capture_worker(
        &self,
        handle: tokio::task::AbortHandle,
        stage: InitialWorkerStage,
    ) {
        if let Self::WorkerProbe(probe) = self {
            let old = probe.0.lock().unwrap().replace((stage, handle));
            drop(old);
        }
    }

    pub(super) async fn apply(
        &self,
        deadline: &mut InitialResponseDeadline,
        bytes: &EndpointByteOwner,
    ) {
        match self {
            Self::None | Self::WorkerProbe(_) => {}
            Self::Commit => deadline.fail_commit(),
            Self::ShrinkOnce => deadline.fail_shrinks(1),
            Self::ShrinkAlways(applied) => {
                deadline.fail_shrinks(usize::MAX);
                // Distinguish persistent shrink failure from earlier finite failures.
                applied.fetch_add(1, Ordering::SeqCst);
            }
            Self::MissingBuffer => drop(deadline.take_buffer()),
            Self::CorruptedBytes => bytes.clone().corrupt_for_test(),
            Self::PauseBeforeCommit(barrier) => barrier.wait().await,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum InitialWorkerStage {
    Validation,
    Setup,
}

/// Reads actual native task completion without polling or aborting its owner.
#[derive(Clone, Default)]
pub(crate) struct InitialWorkerProbe(
    Arc<Mutex<Option<(InitialWorkerStage, tokio::task::AbortHandle)>>>,
);

impl InitialWorkerProbe {
    pub(crate) fn finished(&self, expected: InitialWorkerStage) -> bool {
        self.0
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|(stage, handle)| *stage == expected && handle.is_finished())
    }
}

/// Stops only the test build after callback receipt and before wire publication.
#[derive(Clone, Default)]
pub(crate) struct InitialCommitTestBarrier(Arc<Mutex<BarrierState>>);

#[derive(Default)]
struct BarrierState {
    reached: bool,
    released: bool,
    waker: Option<Waker>,
}

impl InitialCommitTestBarrier {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn reached(&self) -> bool {
        self.0.lock().unwrap().reached
    }

    pub(crate) fn release(&self) {
        let waker = {
            let mut state = self.0.lock().unwrap();
            state.released = true;
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    async fn wait(&self) {
        std::future::poll_fn(|context| {
            let mut state = self.0.lock().unwrap();
            state.reached = true;
            if state.released {
                Poll::Ready(())
            } else {
                state.waker = Some(context.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
}
