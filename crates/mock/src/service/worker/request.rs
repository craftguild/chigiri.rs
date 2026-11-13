//! Own request admission, candidate state, and atomic publication across queued jobs.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::{Client, environment};
use crate::cancellation::Cancellation;
use crate::luau::ScriptError;
use crate::publication::{
    PreparedPublications, PublicationBuffer, PublicationLimits, PublicationStore,
};
use crate::runtime::Execution;
use crate::store::{RequestCandidate, RequestPrepared, Snapshot, Store};
use crate::value::ValueBudget;

#[derive(Clone, Copy, Eq, PartialEq)]
enum State {
    Running,
    Preparing,
    Ready,
    Committed,
    Failed,
}

struct PreparedRequest {
    store: RequestPrepared,
    publications: Option<PreparedPublications>,
}

pub(super) struct Request {
    pub(super) pending: Arc<super::pending::Pending>,
    pub(super) rejections: Arc<super::rejections::Rejections>,
    state: Mutex<State>,
    candidate: Mutex<Option<RequestCandidate>>,
    prepared: Mutex<Option<PreparedRequest>>,
    initial: Snapshot,
    committed: Arc<Mutex<PublicationStore>>,
    publications: Option<PublicationBuffer>,
    cancel: Cancellation,
    shutdown: Cancellation,
    deadline: Instant,
    mutation: bool,
    budget: crate::runtime::ExecutionBudget,
    _permit: Option<tokio::sync::OwnedMutexGuard<()>>,
}

impl Client {
    pub(in crate::service) async fn request_client(
        &self,
        mutation: bool,
        execution: &Execution,
    ) -> Result<Self, ScriptError> {
        let cancelled = || {
            execution.cancel.cancelled()
                || execution.shutdown.cancelled()
                || self.shutdown.cancelled()
        };
        let record_failure = || {
            use crate::limits::request::BudgetFailure;
            let failure = if cancelled() {
                BudgetFailure::Cancelled
            } else if Instant::now() >= execution.deadline {
                BudgetFailure::Deadline
            } else {
                BudgetFailure::Execution
            };
            execution.budget.0.clone().fail_with(failure);
        };
        if cancelled() || Instant::now() >= execution.deadline {
            record_failure();
            return Err(environment("request admission cancelled"));
        }
        let permit = if mutation {
            Some(
                self.mutation_scope(execution)
                    .await
                    .inspect_err(|_| record_failure())?,
            )
        } else {
            None
        };
        if cancelled() || Instant::now() >= execution.deadline {
            record_failure();
            return Err(environment("request admission cancelled"));
        }
        let (candidate, publication_owner) = {
            let committed = self.committed.lock().expect("store lock must be available");
            if mutation {
                committed.mutation_admission().map_err(|_| {
                    execution.budget.0.clone().fail();
                    environment("publication ordering unavailable")
                })?;
            }
            (
                committed.begin_request(ValueBudget::with_work(execution.budget.0.clone())),
                committed.capture_owner(),
            )
        };
        let publications = mutation
            .then(|| publication_owner.buffer(PublicationLimits::default()))
            .transpose()
            .map_err(|_| {
                execution.budget.0.clone().fail();
                environment("publication capture unavailable")
            })?;
        let initial = candidate.snapshot();
        let mut client = self.clone();
        client.session = None;
        client.request = Some(Arc::new(Request {
            pending: Arc::default(),
            rejections: Arc::default(),
            state: Mutex::new(State::Running),
            candidate: Mutex::new(Some(candidate)),
            prepared: Mutex::new(None),
            initial,
            committed: self.committed.clone(),
            publications,
            cancel: execution.cancel.clone(),
            shutdown: self.shutdown.clone(),
            deadline: execution.deadline,
            mutation,
            budget: execution.budget.clone(),
            _permit: permit,
        }));
        Ok(client)
    }

    pub(in crate::service) fn commit_request(&self) -> Result<(), ScriptError> {
        self.request
            .as_ref()
            .ok_or_else(|| environment("request owner missing"))?
            .commit(None)
    }

    pub(in crate::service) fn commit_request_logged(
        &self,
        id: u64,
        logger: &crate::logging::Logger,
    ) -> Result<(), ScriptError> {
        self.request
            .as_ref()
            .ok_or_else(|| environment("request owner missing"))?
            .commit(Some((id, logger)))
    }

    pub(in crate::service) fn abort_request(&self) {
        if let Some(request) = &self.request {
            request.fail();
        }
    }

    /// Read mutable request state after cancellation or delivery failure.
    pub(in crate::service) fn request_committed(&self) -> bool {
        self.request.as_ref().is_some_and(|request| {
            *request.state.lock().expect("request state lock") == State::Committed
        })
    }
}

impl Request {
    pub(super) fn cancelled(&self) -> bool {
        self.cancel.cancelled() || self.shutdown.cancelled()
    }

    pub(super) fn publications(&self) -> Option<PublicationBuffer> {
        self.publications.clone()
    }

    /// Use one short lock for admission and input capture so no job survives a failed prepare step.
    pub fn submit<T>(
        &self,
        preparing: bool,
        submit: impl FnOnce() -> Result<T, ScriptError>,
    ) -> Result<T, ScriptError> {
        let mut state = self.state.lock().expect("request state lock");
        if *state != State::Running {
            let mut error = environment("request admission closed");
            error.cancelled = *state == State::Failed;
            return Err(error);
        }
        if preparing {
            *state = State::Preparing;
        }
        let result = submit();
        if let Err(error) = &result {
            let cause = if error.cancelled {
                crate::limits::request::BudgetFailure::Cancelled
            } else {
                crate::limits::request::BudgetFailure::Execution
            };
            self.budget.0.clone().fail_with(cause);
            *state = State::Failed;
        }
        drop(state);
        if result.is_err() {
            self.fail();
        }
        result
    }

    pub fn snapshot(&self) -> Snapshot {
        if !self.mutation {
            return self.initial.clone();
        }
        self.candidate
            .lock()
            .expect("request candidate")
            .as_ref()
            .map(RequestCandidate::snapshot)
            .unwrap_or_else(|| self.initial.clone())
    }

    pub fn execute<T>(
        &self,
        execute: impl FnOnce(&mut Store) -> Result<T, ScriptError>,
    ) -> Result<T, ScriptError> {
        let result = {
            let mut candidate = self.candidate.lock().expect("request candidate");
            let candidate = candidate
                .as_mut()
                .ok_or_else(|| environment("request candidate closed"))?;
            execute(candidate.store_mut())
        };
        if result.is_err() {
            self.fail();
        }
        result
    }

    /// Record a job failure before cancelling its sibling jobs.
    pub(super) fn fail_with(&self, cause: crate::limits::request::BudgetFailure) {
        self.budget.0.clone().fail_with(cause);
        self.fail();
    }

    /// Reject late failure reports without changing an already committed request.
    pub(super) fn fail_submission(&self, cause: crate::limits::request::BudgetFailure) -> bool {
        let mut state = self.state.lock().expect("request state lock");
        if *state == State::Failed {
            return true;
        }
        if *state == State::Committed {
            return false;
        }
        self.budget.0.clone().fail_with(cause);
        *state = State::Failed;
        drop(state);
        self.cancel.cancel();
        false
    }

    /// Treat an accepted job that produces no reply as an independent failure.
    pub(super) fn fail_reply(&self) {
        let mut state = self.state.lock().expect("request state lock");
        if *state == State::Committed {
            return;
        }
        self.budget.0.clone().fail_independent();
        *state = State::Failed;
        drop(state);
        self.cancel.cancel();
    }

    pub fn fail(&self) {
        // Cancel sibling jobs whether they are running or still queued.
        self.cancel.cancel();
        let mut state = self.state.lock().expect("request state lock");
        if *state != State::Committed {
            *state = State::Failed;
        }
    }

    /// Let the FIFO worker prepare the request candidate after all prior jobs complete.
    pub fn prepare(&self) -> Result<(), ScriptError> {
        if *self.state.lock().expect("request state lock") != State::Preparing {
            return Err(environment("request preparation rejected"));
        }
        let candidate = self
            .candidate
            .lock()
            .expect("request candidate")
            .take()
            .ok_or_else(|| environment("request candidate closed"))?;
        let prepared = candidate
            .prepare()
            .map_err(|_| environment("request candidate invalid"))?;
        let publications = if let Some(buffer) = &self.publications {
            let staging = self
                .committed
                .lock()
                .expect("store lock must be available")
                .staging();
            buffer.prepare(&staging).map_err(|_| {
                self.budget.0.clone().fail();
                environment("publication preparation failed")
            })?
        } else {
            None
        };
        let mut state = self.state.lock().expect("request state lock");
        if *state != State::Preparing {
            drop(state);
            return Err(environment("request preparation cancelled"));
        }
        *self.prepared.lock().expect("prepared request lock") = Some(PreparedRequest {
            store: prepared,
            publications,
        });
        *state = State::Ready;
        Ok(())
    }
}

mod commit;
#[cfg(test)]
mod publication_capacity_tests;
#[cfg(test)]
mod publication_tests;

#[cfg(test)]
#[path = "request_tests.rs"]
mod tests;
