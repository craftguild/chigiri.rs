//! Run scripts on a dedicated worker and commit only after validation and cancellation fences.

#[cfg(test)]
mod api_tests;
#[cfg(test)]
mod authoring_boundaries_tests;
#[cfg(test)]
mod automatic_tests;
pub(crate) mod business;
#[cfg(test)]
mod business_tests;
mod consumer;
mod context;
pub(crate) use consumer::execute_consumer;
#[cfg(test)]
mod embedded_tests;
mod execution;
#[cfg(test)]
mod execution_tests;
mod recovery;
#[cfg(test)]
mod selected_api_tests;
mod stack;
#[cfg(test)]
mod stack_tests;
pub use execution::{
    Execution, ExecutionBudget, execute_nested, execute_nested_with, execute_root,
    execute_root_with,
};
mod hook;
mod hook_functions;
#[cfg(test)]
mod hook_tests;
#[cfg(test)]
mod rejection_tests;
pub use hook::{HookCall, HookLog, HookLogSink, HookOutcome, HookViews, execute_hook};
pub(crate) use subscription_setup::{
    SubscriptionBinding, execute as execute_subscription_setup, execute_consumer_setup,
};
pub(crate) mod input;
mod methods;
#[cfg(test)]
mod model_value_policy_tests;
#[cfg(test)]
mod module_deadline_tests;
#[cfg(test)]
mod module_tests;
#[cfg(test)]
mod native_limit_tests;
#[cfg(test)]
mod ownership_tests;
#[cfg(test)]
mod provenance_tests;
mod publication;
#[cfg(test)]
mod publication_tests;
#[cfg(test)]
mod query_tests;
#[cfg(test)]
mod reference_examples_tests;
#[cfg(test)]
mod relation_tests;
#[cfg(test)]
mod root_scope_tests;
mod state;
#[cfg(test)]
mod storage_tests;
mod subscription_setup;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_values;
#[cfg(test)]
mod time_tests;
mod vm;
use std::cell::RefCell;
use std::rc::Rc;
#[cfg(test)]
use std::sync::Arc;
use std::time::{Duration, Instant};

use chigiri::core::Object;

#[cfg(test)]
use crate::cancellation::Cancellation;
use crate::luau::{Script, ScriptError};
use crate::output::{OutputObject, OutputValue};
use crate::store::Store;
use state::{Host, Session};

pub use vm::{EnvironmentInfo, inspect_environment};

/// Apply the earlier of the phase limit and the request-wide absolute deadline.
pub const VM_TIME: Duration = Duration::from_millis(500);

fn execute(
    script: &Script,
    arguments: &Object,
    parent: Option<&OutputObject>,
    mut session: Session,
    mut store: Option<&mut Store>,
    execution: &Execution,
) -> Result<OutputValue, ScriptError> {
    let cancel = &execution.cancel;
    let shutdown = &execution.shutdown;
    let deadline = execution.deadline.min(Instant::now() + VM_TIME);
    match &mut session {
        Session::Read(reader) => {
            reader.share_work(execution.budget.0.clone());
            reader.execution_guard(cancel, shutdown, deadline);
        }
        Session::Write(transaction) => {
            transaction.execution_guard(cancel, shutdown, deadline);
        }
    }
    validate_arguments(script, arguments, session.values())
        .inspect_err(|_| execution.budget.0.clone().fail())?;
    let clock = execution.budget.clock();
    let state = Rc::new(RefCell::new(Host {
        ir: script.ir().clone(),
        session,
        failed: false,
        cancelled_failure: false,
        cancel_unwinding: false,
        last_line: 0,
        stack: None,
        stack_captures: 0,
        stack_source: script.coordinate().into(),
        calls: 0,
        cancel: cancel.clone(),
        shutdown: shutdown.clone(),
        deadline,
        request_state: execution.state.clone(),
        request: execution.request.clone(),
        clock,
        business: execution.budget.2.clone(),
        business_path: execution.budget.3.path.clone(),
        business_unwinding: false,
        publications: execution.budget.4.clone(),
        hook: None,
    }));
    state
        .borrow_mut()
        .budget()
        .map_err(|_| script.error("budget", "execution cancelled or expired", 1, 1))?;
    let value = recovery::run(
        &execution.budget,
        || script.error("panic", "VM execution failed", 1, 1),
        || vm::run_output(script, arguments, parent, &state),
    )
    .map_err(|mut error| {
        error.stack = state.borrow().stack.clone();
        if error.body == (1, 1) && state.borrow().last_line > 0 {
            let located =
                script.positioned_error(error.phase, error.message, state.borrow().last_line, 1);
            error.body = located.body;
            error.mark_source_position();
        }
        error
    })?;
    // Any callback still retaining the host when the VM exits is an ownership failure.
    let mut host = Rc::try_unwrap(state)
        .map_err(|_| {
            execution.budget.0.clone().fail();
            script.error("host", "VM ownership did not close", 1, 1)
        })?
        .into_inner();
    host.budget().map_err(|_| {
        if host.business_unwinding && !host.failed {
            script.error("business", "business rejection", host.last_line, 1)
        } else {
            script.error(
                "budget",
                "host failure or execution expired",
                host.last_line,
                1,
            )
        }
    })?;
    if execution.budget.business().is_some() {
        return Err(script.error("business", "business rejection", host.last_line, 1));
    }
    let field = script.ir().scripts()[script.id().0].field;
    let invariant = |_| {
        execution.budget.0.clone().fail();
        script.error(
            "invariant",
            "result or store validation failed",
            host.last_line,
            1,
        )
    };
    let (output, prepared) = match host.session {
        Session::Read(mut reader) => (
            reader
                .output_provenance(field, &value.value, &value.provenance)
                .map_err(|failure| {
                    execution.budget.0.clone().fail();
                    script.output_error(field, failure)
                })?,
            None,
        ),
        Session::Write(transaction) => {
            let mut prepared = transaction.prepare().map_err(invariant)?;
            let output = prepared
                .output_provenance(field, &value.value, &value.provenance)
                .map_err(|failure| {
                    execution.budget.0.clone().fail();
                    script.output_error(field, failure)
                })?;
            (output, Some(prepared))
        }
    };
    cancel
        .commit(shutdown, deadline, || {
            if let Some(prepared) = prepared {
                store
                    .as_mut()
                    .ok_or_else(|| {
                        execution.budget.0.clone().fail();
                        script.error("contract", "write store missing", 1, 1)
                    })?
                    .commit(prepared)
                    .map_err(invariant)?;
            }
            Ok(output)
        })
        .ok_or_else(|| {
            // Once the commit fence rejects the operation, record the cause in the shared budget.
            let mut work = execution.budget.0.clone();
            work.guard(cancel, shutdown, deadline);
            let _ = work.charge(0);
            script.error("cancel", "cancelled before commit", host.last_line, 1)
        })?
}

fn validate_arguments(
    script: &Script,
    arguments: &Object,
    budget: &mut crate::value::ValueBudget,
) -> Result<(), ScriptError> {
    let error = || script.error("input", "invalid coerced arguments", 1, 1);
    input::validate(
        script.ir(),
        script.ir().scripts()[script.id().0].field,
        arguments,
        budget,
    )
    .map_err(|_| error())
}

#[cfg(test)]
mod local_limit_tests;

#[cfg(test)]
mod stack_attempt_tests;
