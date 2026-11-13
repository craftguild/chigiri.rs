//! Run one registered callback against a private store and sealed causal input.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use chigiri::core::{Object, Value};

use super::state::{Host, Session};
use super::{Execution, HookLogSink, VM_TIME, recovery, vm};
use crate::api::consumers as contract;
use crate::luau::{ConsumerScript, ScriptError};
use crate::publication::{CapturedEvent, PublicationBuffer, PublicationLimits, PublicationOwner};
use crate::store::{CallScope, Store};
use crate::value::ValueBudget;

/// The caller owns write admission and verifies the event's server owner before
/// entering. Only the supplied private candidate changes here. Its complete
/// store/publication exchange remains the caller's existing outer commit fence.
pub(crate) fn execute_consumer(
    consumer: &ConsumerScript,
    event: &CapturedEvent,
    store: &mut Store,
    execution: &Execution,
    owner: &PublicationOwner,
    log: &HookLogSink,
) -> Result<PublicationBuffer, ScriptError> {
    let script = consumer.script();
    let field = script.ir().scripts()[script.id().0].field;
    let error = |phase, message| {
        execution.budget.0.clone().fail();
        script.error(phase, message, 1, 1)
    };
    if !consumer.registration().enabled()
        || consumer.registration().field() != field
        || !Arc::ptr_eq(script.ir(), store.snapshot().ir())
        || event.depth() > contract::MAX_DEPTH
    {
        return Err(error("contract", "consumer input contract mismatch"));
    }
    // Derive every child depth from the immutable envelope. Authored metadata
    // never enters this decision, including when the payload is republished.
    let publications = owner
        .consumer_buffer(PublicationLimits::default(), event.depth())
        .map_err(|_| error("source", "consumer publication capture unavailable"))?;
    let cancel = event.cancellation().map_or_else(
        || execution.cancel.clone(),
        |source| execution.cancel.linked(source),
    );
    let execution = Execution {
        state: script.initial_state(),
        request: None,
        deadline: execution.deadline.min(Instant::now() + VM_TIME),
        cancel,
        budget: execution
            .budget
            .with_publications(Some(publications.clone())),
        ..execution.clone()
    };
    let mut work = execution.budget.0.clone();
    work.guard(&execution.cancel, &execution.shutdown, execution.deadline);
    let mut values = ValueBudget::with_work(work.clone());
    let input = event
        .project(store.snapshot(), field, &mut values)
        .map_err(|_| error("value", "consumer event projection failed"))?;
    let metadata = metadata(event, &mut values)
        .map_err(|_| error("value", "consumer metadata conversion failed"))?;
    // Store transactions share exactly the write permissions and validation of
    // Mutation roots. The checked ConsumerScript grants this separate phase.
    let mut transaction = store
        .begin_with_work(CallScope::MutationRoot, work)
        .map_err(|_| error("budget", "consumer candidate copy failed"))?;
    transaction.execution_guard(&execution.cancel, &execution.shutdown, execution.deadline);
    let state = Rc::new(RefCell::new(Host {
        ir: script.ir().clone(),
        session: Session::Write(transaction),
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
        request: None,
        clock: execution.budget.clock(),
        business: execution.budget.2.clone(),
        business_path: execution.budget.3.path.clone(),
        business_unwinding: false,
        publications: Some(publications.clone()),
        hook: None,
    }));
    let result = recovery::run(
        &execution.budget,
        || script.error("panic", "consumer VM execution failed", 1, 1),
        || {
            vm::run_consumer(
                script,
                &state,
                &vm::ConsumerInvocation {
                    event: &input,
                    metadata: &metadata,
                    log,
                },
            )
        },
    )
    .map_err(|mut error| {
        let host = state.borrow();
        error.stack = host.stack.clone();
        if error.body == (1, 1) && host.last_line > 0 {
            error.body = script
                .positioned_error(error.phase, error.message, host.last_line, 1)
                .body;
            error.mark_source_position();
        }
        error
    });
    let mut host = Rc::try_unwrap(state)
        .map_err(|_| error("host", "consumer VM ownership did not close"))?
        .into_inner();
    result?;
    host.budget().map_err(|_| {
        if host.business_unwinding && !host.failed {
            error("business", "business rejection")
        } else {
            error("budget", "consumer host failure or expiry")
        }
    })?;
    if execution.budget.business().is_some() {
        return Err(error("business", "business rejection"));
    }
    let Session::Write(transaction) = host.session else {
        return Err(error("invariant", "consumer write candidate missing"));
    };
    let prepared = transaction
        .prepare()
        .map_err(|_| error("invariant", "consumer store validation failed"))?;
    execution
        .cancel
        .commit(&execution.shutdown, execution.deadline, || {
            store.commit(prepared)
        })
        .ok_or_else(|| {
            let mut work = execution.budget.0.clone();
            work.guard(&execution.cancel, &execution.shutdown, execution.deadline);
            let _ = work.charge(0);
            script.error(
                "cancel",
                "consumer cancelled before preparation",
                host.last_line,
                1,
            )
        })?
        .map_err(|_| error("invariant", "consumer candidate preparation failed"))?;
    Ok(publications)
}

fn metadata(
    event: &CapturedEvent,
    values: &mut ValueBudget,
) -> Result<Object, crate::value::ValueError> {
    values.container(1)?;
    values.items(3)?;
    for key in [
        crate::api::subscriptions::TOPIC,
        contract::DEPTH,
        contract::DROPPED_TOTAL,
    ] {
        values.key(key)?;
    }
    let topic = match event.topic() {
        Some(topic) => {
            values.string_node(topic.len(), 2)?;
            Value::from(topic)
        }
        None => Value::Null,
    };
    let dropped = match event.dropped_total() {
        Some(total) => Value::Int(i128::from(total)),
        None => Value::Null,
    };
    Ok(Object::from_iter([
        (crate::api::subscriptions::TOPIC, topic),
        (contract::DEPTH, Value::Int(i128::from(event.depth()))),
        (contract::DROPPED_TOTAL, dropped),
    ]))
}
