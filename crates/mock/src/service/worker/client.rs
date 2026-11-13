//! Unifies owned-job admission, queue saturation, absolute deadlines, and future
//! drops under one cancellation path.

use std::time::Instant;

use chigiri::core::Object;
use tokio::sync::oneshot;

use super::jobs::{Job, Work};
use super::{Client, environment};
use crate::cancellation::{CancelOnDrop, Cancellation};
use crate::hooks::{HookSlot, RequestState};
use crate::ir::{FieldId, MockIr};
use crate::luau::ScriptError;
use crate::output::{OutputObject, OutputValue};
use crate::runtime::{Execution, HookCall, HookOutcome};
use crate::store::Snapshot;

#[cfg(test)]
#[path = "client/publication_logging_tests.rs"]
mod publication_logging_tests;
#[cfg(test)]
#[path = "client/publication_tests.rs"]
mod publication_tests;

/// Retains the snapshot observed by `field.after`, including scalar successes and
/// failures.
pub struct Resolution {
    /// Original resolver result. The request owner aggregates phase failures.
    pub output: Result<OutputValue, ScriptError>,
    /// Generation read by queries and nested fields, or the private candidate
    /// generation after a mutation root runs.
    pub snapshot: Snapshot,
}

impl Client {
    /// IR shared with this worker.
    pub fn ir(&self) -> &std::sync::Arc<MockIr> {
        &self.ir
    }

    /// Create fresh initial state for a request; mutable values are never shared
    /// across requests.
    pub fn initial_state(&self) -> RequestState {
        self.initial.instantiate()
    }

    /// Whether a slot is registered, allowing disabled slots to avoid queueing and
    /// VM initialization.
    pub fn enabled(&self, slot: HookSlot) -> bool {
        self.hooks.contains(&slot)
    }

    /// Preserve mutation write order from request start through commit or discard,
    /// without retaining a VM lease.
    pub(super) async fn mutation_scope(
        &self,
        execution: &Execution,
    ) -> Result<tokio::sync::OwnedMutexGuard<()>, ScriptError> {
        if let Ok(permit) = self.mutation.clone().try_lock_owned() {
            return Ok(permit);
        }
        // Bound waiting futures in addition to the current owner, using the
        // existing queue limit.
        let _waiting = self
            .mutation_waiters
            .clone()
            .try_acquire_owned()
            .map_err(|_| environment("mutation admission full"))?;
        tokio::select! {
            biased;
            _ = execution.cancel.wait() => Err(environment("mutation admission cancelled")),
            _ = execution.shutdown.wait() => Err(environment("mutation admission stopped")),
            _ = self.shutdown.wait() => Err(environment("mutation admission stopped")),
            result = tokio::time::timeout_at(execution.deadline.into(), self.mutation.clone().lock_owned()) => {
                result.map_err(|_| environment("mutation admission expired"))
            }
        }
    }

    /// Return the request's fixed candidate generation when in a request, or the
    /// worker's committed generation otherwise.
    pub async fn snapshot(&self, deadline: Instant) -> Result<Snapshot, ScriptError> {
        if let Some(request) = &self.request {
            return Ok(request.snapshot());
        }
        self.submit(deadline, |reply, _| Work::Snapshot(reply))
            .await?
    }

    /// Send a hook to the dedicated worker. A disabled slot creates no VM job and
    /// continues with `nil`.
    pub async fn hook(
        &self,
        slot: HookSlot,
        mut call: HookCall,
    ) -> Result<HookOutcome, ScriptError> {
        if !self.enabled(slot) {
            return Ok(HookOutcome::Continue(None));
        }
        call.shutdown = self.shutdown.clone();
        self.submit(call.deadline, |reply, cancel| {
            call.cancel = cancel.linked(&call.cancel);
            Work::Hook { slot, call, reply }
        })
        .await?
    }

    /// Run an independent resolver with the same queue, initial state, and deadline.
    pub async fn execute(
        &self,
        field: FieldId,
        arguments: Object,
        parent: Option<OutputObject>,
    ) -> Result<OutputValue, ScriptError> {
        let deadline = Instant::now() + crate::limits::OPERATION_TIME;
        let budget = crate::runtime::ExecutionBudget::default();
        budget.set_clock(&self.clock);
        let execution = Execution {
            state: self.initial_state().snapshot(),
            request: None,
            deadline,
            cancel: Cancellation::default(),
            shutdown: self.shutdown.clone(),
            budget,
        };
        let _cancel = CancelOnDrop(execution.cancel.clone());
        let client = self
            .request_client(self.is_mutation(field, parent.is_none()), &execution)
            .await?;
        let output = client
            .execute_with(field, arguments, parent, None, execution)
            .await?
            .output?;
        client.prepare_request(deadline).await?;
        client
            .request
            .as_ref()
            .expect("request owner")
            .commit(None)?;
        Ok(output)
    }

    /// Run a resolver with plugin-selected state and snapshot, returning the same
    /// generation with the outcome.
    /// For mutations, the request owner holds the permit through all hooks,
    /// encoding, and the final swap.
    pub async fn execute_with(
        &self,
        field: FieldId,
        arguments: Object,
        parent: Option<OutputObject>,
        snapshot: Option<Snapshot>,
        mut execution: Execution,
    ) -> Result<Resolution, ScriptError> {
        execution.shutdown = self.shutdown.clone();
        execution.budget = execution.budget.with_publications(
            self.request
                .as_ref()
                .and_then(|request| request.publications()),
        );
        self.submit(execution.deadline, |reply, cancel| {
            execution.cancel = cancel.linked(&execution.cancel);
            Work::Resolve {
                field,
                arguments,
                parent,
                snapshot,
                execution,
                request: self.request.clone(),
                reply,
            }
        })
        .await?
    }

    #[cfg(test)]
    pub(in crate::service) async fn subscription_setup(
        &self,
        field: FieldId,
        arguments: Object,
        snapshot: Option<Snapshot>,
        mut execution: Execution,
    ) -> Result<crate::runtime::SubscriptionBinding, ScriptError> {
        execution.shutdown = self.shutdown.clone();
        self.submit(execution.deadline, |reply, cancel| {
            execution.cancel = cancel.linked(&execution.cancel);
            Work::SubscriptionSetup {
                field,
                arguments,
                snapshot,
                execution,
                reply,
            }
        })
        .await?
    }

    pub(in crate::service) async fn establish_subscription(
        &self,
        field: FieldId,
        arguments: Object,
        snapshot: Option<Snapshot>,
        mut execution: Execution,
        observation: Option<super::super::hooks::RegistrationFence>,
    ) -> Result<super::super::hooks::EstablishedSubscription, ScriptError> {
        execution.shutdown = self.shutdown.clone();
        self.submit(execution.deadline, |reply, cancel| {
            execution.cancel = cancel.linked(&execution.cancel);
            Work::EstablishSubscription {
                field,
                arguments,
                snapshot,
                execution,
                observation,
                reply,
            }
        })
        .await?
    }

    /// Reject an event from another worker before projection can consume output budget.
    pub(in crate::service) fn validate_subscription_event(
        &self,
        event: &crate::publication::CapturedEvent,
    ) -> Result<(), ScriptError> {
        let committed = self
            .committed
            .lock()
            .map_err(|_| environment("publication owner poisoned"))?;
        if committed.owns_event(event) {
            Ok(())
        } else {
            Err(ScriptError::new(
                "contract",
                "publication event belongs to another worker",
            ))
        }
    }

    pub(super) fn is_mutation(&self, field: FieldId, root: bool) -> bool {
        root && self
            .ir
            .graph()
            .fields()
            .get(field.0)
            .is_some_and(|field| Some(field.parent) == self.ir.graph().mutation())
    }

    async fn submit<T>(
        &self,
        deadline: Instant,
        work: impl FnOnce(oneshot::Sender<T>, Cancellation) -> Work,
    ) -> Result<T, ScriptError> {
        if self.shutdown.cancelled() {
            return Err(self.submission_error(
                crate::limits::request::BudgetFailure::Cancelled,
                "worker request cancelled",
            ));
        }
        if Instant::now() >= deadline {
            return Err(self.submission_error(
                crate::limits::request::BudgetFailure::Deadline,
                "worker request expired",
            ));
        }
        let mut cancel = JobOwner {
            token: Cancellation::default(),
            request: self.request.clone(),
            pending: false,
        };
        let (reply, receive) = oneshot::channel();
        let job = Job {
            cancel: cancel.token.clone(),
            deadline,
            request: self.request.clone(),
            work: work(reply, cancel.token.clone()),
            _ticket: self.request.as_ref().map(|request| {
                let mut ticket = request.pending.ticket(Some(request.clone()));
                ticket.diagnostics = self.diagnostics.clone();
                ticket
            }),
        };
        let preparing = matches!(&job.work, Work::Prepare { .. });
        let send = || self.enqueue(job);
        if let Some(request) = &self.request {
            request.submit(preparing, send)?;
        } else {
            send()?;
        }
        cancel.pending = true;
        let result = tokio::time::timeout_at(deadline.into(), receive)
            .await
            .map_err(|_| {
                self.submission_error(
                    crate::limits::request::BudgetFailure::Deadline,
                    "worker request expired",
                )
            })?
            .map_err(|_| {
                if let Some(request) = &self.request {
                    request.fail_reply();
                }
                super::reply_missing()
            });
        cancel.pending = result.is_err();
        result
    }

    /// Order admission against shutdown without awaiting or acquiring cancellation
    /// or request locks.
    pub(super) fn enqueue(&self, mut job: Job) -> Result<(), ScriptError> {
        let open = self.queue_open.lock().expect("worker admission lock");
        if !*open {
            let mut error = environment("worker admission closed");
            error.cancelled = true;
            return Err(error);
        }
        if let Some(ticket) = &mut job._ticket {
            ticket.accept();
        }
        self.sender.try_send(job).map_err(|error| {
            let mut job = error.into_inner();
            if let Some(ticket) = &mut job._ticket {
                ticket.complete();
            }
            environment("worker queue unavailable")
        })
    }

    /// Preserve the typed admission or reply failure before `JobOwner` cancellation.
    fn submission_error(
        &self,
        cause: crate::limits::request::BudgetFailure,
        message: &'static str,
    ) -> ScriptError {
        let cleanup = self
            .request
            .as_ref()
            .is_some_and(|request| request.fail_submission(cause));
        let mut error = environment(message);
        error.cancelled =
            cleanup || matches!(cause, crate::limits::request::BudgetFailure::Cancelled);
        error
    }

    pub(in crate::service) async fn prepare_request(
        &self,
        deadline: Instant,
    ) -> Result<(), ScriptError> {
        let request = self
            .request
            .clone()
            .ok_or_else(|| environment("request owner missing"))?;
        self.submit(deadline, |reply, _| Work::Prepare { request, reply })
            .await?
    }
}

/// Propagates child-future cancellation into the request and prevents jobs without
/// an acknowledgement from being treated as successful.
struct JobOwner {
    token: Cancellation,
    request: Option<std::sync::Arc<super::request::Request>>,
    pending: bool,
}

impl Drop for JobOwner {
    fn drop(&mut self) {
        self.token.cancel();
        if self.pending
            && let Some(request) = &self.request
        {
            request.fail();
        }
    }
}

#[cfg(test)]
#[path = "client/publication_inspection.rs"]
mod publication_inspection;
