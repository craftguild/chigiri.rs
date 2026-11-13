//! Reject accepted jobs and retire replies outside the worker's admission lock.

use std::sync::Arc;

use tokio::sync::oneshot;

use super::{Job, Work};
use crate::cancellation::Cancellation;
use crate::luau::ScriptError;

/// Observe linked request cancellation before running queued user or native work.
pub(super) fn cancelled(job: &Job, shutdown: &Cancellation) -> bool {
    job.cancel.cancelled()
        || shutdown.cancelled()
        || job
            .request
            .as_ref()
            .is_some_and(|request| request.cancelled())
        || match &job.work {
            Work::Hook { call, .. } => call.cancel.cancelled(),
            Work::Resolve { execution, .. } | Work::EstablishSubscription { execution, .. } => {
                execution.cancel.cancelled()
            }
            #[cfg(test)]
            Work::SubscriptionSetup { execution, .. } => execution.cancel.cancelled(),
            Work::Prepare { request, .. } => request.cancelled(),
            Work::Snapshot(_) => false,
        }
}

/// Deliver a cause only to running work; a closed oneshot is not an internal failure.
pub(super) fn reject(mut job: Job, cancelled: bool) {
    use crate::limits::request::BudgetFailure;
    let cause = if cancelled {
        BudgetFailure::Cancelled
    } else {
        BudgetFailure::Deadline
    };
    let mut failure = ScriptError::new("budget", "worker job cancelled or expired");
    failure.cancelled = cancelled;
    match &job.work {
        Work::Hook { call, .. } => call.budget.0.clone().fail_with(cause),
        Work::Resolve { execution, .. } | Work::EstablishSubscription { execution, .. } => {
            execution.budget.0.clone().fail_with(cause)
        }
        #[cfg(test)]
        Work::SubscriptionSetup { execution, .. } => execution.budget.0.clone().fail_with(cause),
        Work::Prepare { request, .. } => request.fail_with(cause),
        Work::Snapshot(_) => {}
    }
    if let Some(request) = &job.request {
        request.fail_with(cause);
    }
    report(&job._ticket, &failure);
    match job.work {
        Work::Snapshot(reply) => deliver(reply, Err(failure), job.request.as_ref()),
        Work::Prepare { reply, .. } => deliver(reply, Err(failure), job.request.as_ref()),
        Work::Hook { reply, .. } => deliver(reply, Err(failure), job.request.as_ref()),
        Work::Resolve { reply, .. } => deliver(reply, Err(failure), job.request.as_ref()),
        #[cfg(test)]
        Work::SubscriptionSetup { reply, .. } => deliver(reply, Err(failure), job.request.as_ref()),
        Work::EstablishSubscription { reply, .. } => {
            deliver(reply, Err(failure), job.request.as_ref())
        }
    }
    if let Some(ticket) = &mut job._ticket {
        ticket.complete();
    }
}

pub(super) fn report(ticket: &Option<super::super::pending::Ticket>, error: &ScriptError) {
    if let Some(ticket) = ticket {
        ticket.report(error);
    }
}

pub(super) fn deliver<T>(
    reply: oneshot::Sender<T>,
    value: T,
    request: Option<&Arc<super::super::request::Request>>,
) {
    if reply.send(value).is_err()
        && let Some(request) = request
    {
        request.fail();
    }
}

#[cfg(test)]
mod tests;
