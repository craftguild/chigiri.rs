//! Run snapshot hook and resolver work through one queue with shared deadlines and cancellation.

use std::sync::Arc;
#[cfg(test)]
use std::time::Duration;
use std::time::Instant;

use chigiri::core::Object;
use tokio::sync::oneshot;

use super::client::Resolution;
use crate::cancellation::Cancellation;
use crate::hooks::HookSlot;
use crate::ir::{FieldId, FieldPlan};
use crate::luau::{HookScript, ScriptError, ScriptSet};
use crate::output::{OutputObject, OutputValue};
use crate::runtime::{self, Execution, HookCall, HookOutcome};
use crate::store::{Snapshot, Store};

mod rejection;
mod scheduler;
mod subscription;

use rejection::{deliver, reject, report};
pub(super) use scheduler::run;

pub(super) struct Job {
    pub cancel: Cancellation,
    pub deadline: Instant,
    /// Keep the application candidate and the mutation permit until the end of the job, including hook.
    pub request: Option<Arc<super::request::Request>>,
    pub work: Work,
    /// Delivery guard that cancels undelivered work when dropped.
    pub _ticket: Option<super::pending::Ticket>,
}

pub(super) enum Work {
    Snapshot(oneshot::Sender<Result<Snapshot, ScriptError>>),
    Prepare {
        request: Arc<super::request::Request>,
        reply: oneshot::Sender<Result<(), ScriptError>>,
    },
    Hook {
        slot: HookSlot,
        call: HookCall,
        reply: oneshot::Sender<Result<HookOutcome, ScriptError>>,
    },
    Resolve {
        field: FieldId,
        arguments: Object,
        parent: Option<OutputObject>,
        snapshot: Option<Snapshot>,
        execution: Execution,
        request: Option<Arc<super::request::Request>>,
        reply: oneshot::Sender<Result<Resolution, ScriptError>>,
    },
    #[cfg(test)]
    SubscriptionSetup {
        field: FieldId,
        arguments: Object,
        snapshot: Option<Snapshot>,
        execution: Execution,
        reply: oneshot::Sender<Result<runtime::SubscriptionBinding, ScriptError>>,
    },
    EstablishSubscription {
        field: FieldId,
        arguments: Object,
        snapshot: Option<Snapshot>,
        execution: Execution,
        observation: Option<super::super::hooks::RegistrationFence>,
        reply: oneshot::Sender<Result<super::super::hooks::EstablishedSubscription, ScriptError>>,
    },
}

fn execute_job(
    store: &Arc<std::sync::Mutex<crate::publication::PublicationStore>>,
    scripts: &ScriptSet,
    hooks: &[HookScript],
    mut job: Job,
    shutdown: &Cancellation,
) {
    let cancelled = rejection::cancelled(&job, shutdown);
    if cancelled || Instant::now() >= job.deadline {
        reject(job, cancelled);
        return;
    }
    match job.work {
        Work::Snapshot(reply) => {
            let snapshot = store
                .lock()
                .expect("store lock must be available")
                .snapshot();
            deliver(reply, Ok(snapshot), job.request.as_ref());
        }
        Work::Prepare { request, reply } => {
            let result = request.prepare();
            if result.is_err() {
                request.fail();
            }
            deliver(reply, result, job.request.as_ref());
        }
        Work::Hook { slot, call, reply } => {
            let result = hooks
                .iter()
                .find(|hook| hook.slot() == slot)
                .ok_or_else(|| {
                    call.budget.0.clone().fail();
                    ScriptError::new("contract", "worker hook missing")
                })
                .and_then(|script| runtime::execute_hook(script, call));
            if result.is_err()
                && let Some(request) = &job.request
            {
                request.fail();
            }
            if let Err(error) = &result {
                report(&job._ticket, error);
            }
            if let Ok(HookOutcome::Rejected(rejection, _)) = &result
                && let Some(request) = &job.request
            {
                request.rejections.record(rejection);
            }
            if let Ok(HookOutcome::Rejected(_, Some(error))) = &result {
                report(&job._ticket, error);
            }
            deliver(reply, result, job.request.as_ref());
        }
        Work::Resolve {
            field,
            arguments,
            parent,
            snapshot,
            execution,
            request,
            reply,
        } => {
            let snapshot = parent
                .as_ref()
                .map(|parent| parent.snapshot().clone())
                .or(snapshot)
                .unwrap_or_else(|| {
                    request
                        .as_ref()
                        .map(|request| request.snapshot())
                        .unwrap_or_else(|| {
                            store
                                .lock()
                                .expect("store lock must be available")
                                .snapshot()
                        })
                });
            let mutation = parent.is_none()
                && scripts
                    .contract()
                    .ir()
                    .graph()
                    .fields()
                    .get(field.0)
                    .is_some_and(|field| {
                        Some(field.parent) == scripts.contract().ir().graph().mutation()
                    });
            let run = |candidate: &mut Store| {
                execute(
                    candidate,
                    scripts,
                    field,
                    &arguments,
                    parent.as_ref(),
                    &snapshot,
                    &execution,
                )
            };
            let (output, snapshot) = if let Some(request) = request {
                let output = request.execute(run);
                let snapshot = if mutation {
                    request.snapshot()
                } else {
                    snapshot
                };
                (output, snapshot)
            } else {
                let mut candidate = store
                    .lock()
                    .expect("store lock must be available")
                    .begin_request(crate::value::ValueBudget::with_work(
                        execution.budget.0.clone(),
                    ));
                let output = run(candidate.store_mut());
                let snapshot = if mutation {
                    candidate.snapshot()
                } else {
                    snapshot
                };
                let output = output.and_then(|output| {
                    if mutation {
                        let prepared = candidate
                            .prepare()
                            .map_err(|_| super::environment("candidate invalid"))?;
                        execution
                            .cancel
                            .commit(&execution.shutdown, execution.deadline, || {
                                store
                                    .lock()
                                    .expect("store lock must be available")
                                    .commit_request(&prepared)
                            })
                            .ok_or_else(|| super::environment("commit cancelled"))?
                            .map_err(|_| super::environment("commit rejected"))?;
                    }
                    Ok(output)
                });
                (output, snapshot)
            };
            if let Err(error) = &output {
                report(&job._ticket, error);
            }
            deliver(
                reply,
                Ok(Resolution { output, snapshot }),
                job.request.as_ref(),
            );
        }
        #[cfg(test)]
        Work::SubscriptionSetup {
            field,
            arguments,
            snapshot,
            execution,
            reply,
        } => {
            let result = subscription::setup(
                store,
                scripts,
                job.request.as_ref(),
                field,
                &arguments,
                snapshot,
                &execution,
            );
            if let Err(error) = &result {
                report(&job._ticket, error);
            }
            deliver(reply, result, job.request.as_ref());
        }
        Work::EstablishSubscription {
            field,
            arguments,
            snapshot,
            execution,
            observation,
            reply,
        } => {
            let result = subscription::establish(
                store,
                scripts,
                subscription::EstablishInput::new(
                    job.request.as_ref(),
                    field,
                    &arguments,
                    snapshot,
                    &execution,
                    observation,
                ),
            );
            if let Err(error) = &result {
                report(&job._ticket, error);
            }
            deliver(reply, result, job.request.as_ref());
        }
    }
    if let Some(ticket) = &mut job._ticket {
        ticket.complete();
    }
}

fn execute(
    store: &mut Store,
    scripts: &ScriptSet,
    field_id: FieldId,
    arguments: &Object,
    parent: Option<&OutputObject>,
    snapshot: &Snapshot,
    execution: &Execution,
) -> Result<OutputValue, ScriptError> {
    let error = |phase, message| {
        execution.budget.0.clone().fail();
        ScriptError::new(phase, message)
    };
    let ir = scripts.contract().ir();
    let field = ir
        .graph()
        .fields()
        .get(field_id.0)
        .ok_or_else(|| error("contract", "unknown worker field"))?;
    if !Arc::ptr_eq(ir, snapshot.ir()) {
        return Err(error("contract", "worker snapshot schema differs"));
    }
    if let Some(parent) = parent {
        if !Arc::ptr_eq(ir, parent.snapshot().ir()) || parent.ty() != field.parent {
            return Err(error("contract", "worker parent and field differ"));
        }
    } else if field.parent != ir.graph().query() && Some(field.parent) != ir.graph().mutation() {
        return Err(error("contract", "nested worker field requires parent"));
    }
    match ir.field_plan(field_id) {
        Some(FieldPlan::Script(id)) => {
            let script = &scripts.scripts()[id.0];
            if let Some(parent) = parent {
                runtime::execute_nested_with(script, arguments, parent, execution)
            } else {
                runtime::execute_root_with(script, arguments, store, snapshot, execution)
            }
        }
        Some(_) => {
            let parent = parent.ok_or_else(|| error("contract", "native field requires parent"))?;
            let mut execution = execution.clone();
            execution.deadline = execution.deadline.min(Instant::now() + runtime::VM_TIME);
            let output = super::super::native::execute(field_id, arguments, parent, &execution)
                .map_err(|failure| match failure {
                    super::super::native::NativeError::Output(failure) => {
                        execution.budget.0.clone().fail();
                        ScriptError::output(ir, field_id, failure)
                    }
                    super::super::native::NativeError::Other => {
                        error("invariant", "native field validation failed")
                    }
                })?;
            execution
                .cancel
                .commit(&execution.shutdown, execution.deadline, || output)
                .ok_or_else(|| {
                    // After the commit fence rejects, apply the same cancellation and deadline guard to the shared ledger.
                    let mut work = execution.budget.0.clone();
                    work.guard(&execution.cancel, &execution.shutdown, execution.deadline);
                    let _ = work.charge(0);
                    error("cancel", "native field cancelled")
                })
        }
        None => Err(error("contract", "concrete worker field required")),
    }
}

#[cfg(test)]
#[path = "jobs_tests.rs"]
mod tests;
