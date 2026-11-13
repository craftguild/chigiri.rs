//! Actual join completion or charged retirement for one admitted HTTP request.

use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};

use chigiri_http::ResponseMediaType;
use tokio::sync::oneshot;
use tokio::time::Instant;

use super::Disconnect;
use crate::managed::control::RequestPermit;
use crate::managed::{Control, ManagedHttpResponse, StopPhase};
use crate::subscription::worker::{
    ConcreteWorker, ConcreteWorkerOutcome, RetiredWorkerOwner, WorkerGenerationSlot,
    WorkerRetainedState,
};

pub(super) struct Entry {
    pub(super) worker: Option<ConcreteWorker<ManagedHttpResponse>>,
    pub(super) slot: Option<WorkerGenerationSlot>,
    pub(super) retained: Option<RetainedHttpRequest>,
    pub(super) response: Option<oneshot::Sender<ManagedHttpResponse>>,
    pub(super) disconnect: Arc<Disconnect>,
    pub(super) control: Arc<Control>,
    pub(super) finished: Arc<OnceLock<Instant>>,
    pub(super) deadline: Instant,
    pub(super) media: ResponseMediaType,
}

pub(crate) struct RetainedHttpRequest {
    pub(super) deadlines: crate::transport::deadline::HttpDeadlineState,
    pub(super) _permit: RequestPermit,
}

impl RetainedHttpRequest {
    pub(crate) fn is_poisoned(&self) -> bool {
        self.deadlines
            .poisoned_flag()
            .load(std::sync::atomic::Ordering::Acquire)
    }
}

impl Entry {
    pub(super) fn poll(&mut self, context: &mut Context<'_>) -> bool {
        let phase = self.control.phase();
        let unselected = matches!(phase, StopPhase::Graceful { .. })
            && !self
                .retained
                .as_ref()
                .expect("HTTP request owners")
                .deadlines
                .finite_admitted();
        if phase == StopPhase::Cancelled || unselected {
            self.retained
                .as_ref()
                .expect("HTTP request owners")
                .deadlines
                .cancel(self.control.outcome() == Some(crate::ShutdownOutcome::Expired));
            self.respond(super::response::cancelled(&self.control));
            return true;
        }
        if self.disconnect.is_closed() {
            self.retained
                .as_ref()
                .expect("HTTP request owners")
                .deadlines
                .cancel(false);
            return true;
        }
        let completed_in_time = self.finished.get().is_some_and(|at| *at < self.deadline);
        if !completed_in_time {
            let timeout = self
                .retained
                .as_mut()
                .expect("HTTP request owners")
                .deadlines
                .poll_timeout(context, self.media);
            if let Poll::Ready(result) = timeout {
                let response = match result {
                    Ok(parts) => ManagedHttpResponse::Finite(parts),
                    Err(_) => {
                        self.control.fail();
                        ManagedHttpResponse::internal_failure()
                    }
                };
                self.respond(response);
                return true;
            }
        }
        let outcome = match self
            .worker
            .as_mut()
            .expect("HTTP concrete worker")
            .poll_http(
                context,
                self.retained
                    .as_ref()
                    .expect("HTTP request owners")
                    .deadlines
                    .poisoned_flag(),
            ) {
            Poll::Pending => return false,
            Poll::Ready(outcome) => outcome,
        };
        let response = match outcome {
            ConcreteWorkerOutcome::Completed(response) => response,
            ConcreteWorkerOutcome::Poisoned => {
                self.control.fail();
                self.respond(ManagedHttpResponse::internal_failure());
                return true;
            }
            ConcreteWorkerOutcome::Cancelled | ConcreteWorkerOutcome::Panicked => {
                self.control.fail();
                ManagedHttpResponse::internal_failure()
            }
        };
        self.worker.take();
        self.slot.take();
        // A ready native response alone does not release this count. Only an
        // actual joined worker reaches this point; server-owned bytes may remain.
        self.retained.take();
        self.respond(response);
        true
    }

    fn respond(&mut self, response: ManagedHttpResponse) {
        if let Some(sender) = self.response.take() {
            let _ = sender.send(response);
        }
    }
}

impl Drop for Entry {
    fn drop(&mut self) {
        let Some(worker) = self.worker.take() else {
            return;
        };
        if let Some(retained) = &self.retained {
            retained
                .deadlines
                .cancel(self.control.outcome() == Some(crate::ShutdownOutcome::Expired));
        }
        worker.abort();
        if let Some(slot) = self.slot.take() {
            slot.transfer();
        }
        if let Some(retained) = self.retained.take() {
            worker.erase().retire(RetiredWorkerOwner::orphan(
                WorkerRetainedState::ManagedHttp {
                    _retained: Box::new(retained),
                },
            ));
        } else {
            // No physical request owner may be synthesized after corruption.
            self.control.fail();
            std::mem::forget(worker);
        }
    }
}
