//! Reuse the ordinary request budgets while keeping setup read-only and VM-local.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use chigiri::core::Object;

use super::{BindingCollector, SubscriptionBinding};
use crate::ir::ScriptRole;
use crate::luau::{ConsumerScript, Script, ScriptError};
use crate::runtime::state::{Host, Session};
use crate::runtime::{Execution, HookLogSink, VM_TIME};
use crate::store::Snapshot;

pub(crate) fn execute(
    script: &Script,
    arguments: &Object,
    snapshot: &Snapshot,
    execution: &Execution,
    source_limits: crate::events::SourceLimits,
    retention: crate::events::retention::Retention,
) -> Result<SubscriptionBinding, ScriptError> {
    run(
        script,
        arguments,
        snapshot,
        execution,
        source_limits,
        retention,
        None,
    )
}

pub(crate) fn execute_consumer_setup(
    consumer: &ConsumerScript,
    snapshot: &Snapshot,
    execution: &Execution,
    source_limits: crate::events::SourceLimits,
    retention: crate::events::retention::Retention,
    log: &HookLogSink,
) -> Result<SubscriptionBinding, ScriptError> {
    let script = consumer.script();
    if !consumer.registration().enabled() {
        execution.budget.0.clone().fail();
        return Err(script.error("contract", "disabled consumer cannot execute", 1, 1));
    }
    let execution = Execution {
        state: script.initial_state(),
        request: None,
        ..execution.clone()
    };
    run(
        script,
        &Object::new(),
        snapshot,
        &execution,
        source_limits,
        retention,
        Some(log),
    )
}

fn run(
    script: &Script,
    arguments: &Object,
    snapshot: &Snapshot,
    execution: &Execution,
    source_limits: crate::events::SourceLimits,
    retention: crate::events::retention::Retention,
    log: Option<&HookLogSink>,
) -> Result<SubscriptionBinding, ScriptError> {
    let plan = &script.ir().scripts()[script.id().0];
    if !matches!(plan.role, ScriptRole::SubscriptionRoot { .. })
        || !Arc::ptr_eq(script.ir(), snapshot.ir())
        || script.consumer_registration().is_some() != log.is_some()
    {
        execution.budget.0.clone().fail();
        return Err(script.error("contract", "subscription setup contract mismatch", 1, 1));
    }
    let execution = Execution {
        deadline: execution.deadline.min(Instant::now() + VM_TIME),
        ..execution.clone()
    };
    let mut reader = snapshot.reader();
    reader.share_work(execution.budget.0.clone());
    reader.execution_guard(&execution.cancel, &execution.shutdown, execution.deadline);
    if log.is_none() {
        super::super::validate_arguments(script, arguments, reader.value_budget())
            .inspect_err(|_| execution.budget.0.clone().fail())?;
    }
    let state = Rc::new(RefCell::new(Host {
        ir: script.ir().clone(),
        session: Session::Read(reader),
        failed: false,
        cancelled_failure: false,
        cancel_unwinding: false,
        last_line: 0,
        stack: None,
        stack_captures: 0,
        stack_source: script.coordinate().into(),
        calls: 0,
        cancel: execution.cancel.clone(),
        shutdown: execution.shutdown.clone(),
        deadline: execution.deadline,
        request_state: execution.state.clone(),
        request: execution.request.clone(),
        clock: execution.budget.clock(),
        business: execution.budget.2.clone(),
        business_path: execution.budget.3.path.clone(),
        business_unwinding: false,
        publications: None,
        hook: None,
    }));
    state
        .borrow_mut()
        .budget()
        .map_err(|_| script.error("budget", "setup cancelled or expired", 1, 1))?;
    let bindings = BindingCollector::with_policy(script.id(), source_limits, retention);
    let bindings = if log.is_some() {
        bindings.for_consumer()
    } else {
        bindings
    };
    let result = super::super::recovery::run(
        &execution.budget,
        || script.error("panic", "subscription setup failed", 1, 1),
        || super::super::vm::run_subscription(script, arguments, &state, &bindings, log),
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
    });
    if !bindings.closed() {
        execution.budget.0.clone().fail();
        return Err(script.error("host", "setup binding ownership did not close", 1, 1));
    }
    let mut host = Rc::try_unwrap(state)
        .map_err(|_| {
            execution.budget.0.clone().fail();
            script.error("host", "setup VM ownership did not close", 1, 1)
        })?
        .into_inner();
    let binding = result?;
    host.budget()
        .map_err(|_| script.error("budget", "setup failure or expiry", host.last_line, 1))?;
    if execution.budget.business().is_some() {
        return Err(script.error("business", "business rejection", host.last_line, 1));
    }
    execution
        .cancel
        .commit(&execution.shutdown, execution.deadline, || binding)
        .ok_or_else(|| {
            let mut work = execution.budget.0.clone();
            work.guard(&execution.cancel, &execution.shutdown, execution.deadline);
            let _ = work.charge(0);
            script.error("cancel", "subscription setup cancelled", host.last_line, 1)
        })
}
