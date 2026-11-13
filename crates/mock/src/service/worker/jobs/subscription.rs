//! Establish one source after setup without retaining VM values across the store fence.

use std::sync::{Arc, Mutex};

use chigiri::core::Object;

use super::super::request::Request;
use crate::ir::{FieldId, FieldPlan};
use crate::luau::{ScriptError, ScriptSet};
use crate::publication::{PublicationError, PublicationStore};
use crate::runtime::{self, Execution, SubscriptionBinding};
use crate::service::hooks::{EstablishedSubscription, RegistrationFence};
use crate::store::Snapshot;

pub(super) struct EstablishInput<'a> {
    request: Option<&'a Arc<Request>>,
    field: FieldId,
    arguments: &'a Object,
    snapshot: Option<Snapshot>,
    execution: &'a Execution,
    observation: Option<RegistrationFence>,
}

impl<'a> EstablishInput<'a> {
    pub(super) const fn new(
        request: Option<&'a Arc<Request>>,
        field: FieldId,
        arguments: &'a Object,
        snapshot: Option<Snapshot>,
        execution: &'a Execution,
        observation: Option<RegistrationFence>,
    ) -> Self {
        Self {
            request,
            field,
            arguments,
            snapshot,
            execution,
            observation,
        }
    }
}

pub(super) fn setup(
    store: &Arc<Mutex<PublicationStore>>,
    scripts: &ScriptSet,
    request: Option<&Arc<Request>>,
    field: FieldId,
    arguments: &Object,
    snapshot: Option<Snapshot>,
    execution: &Execution,
) -> Result<SubscriptionBinding, ScriptError> {
    let snapshot = snapshot.unwrap_or_else(|| {
        request
            .map(|request| request.snapshot())
            .unwrap_or_else(|| {
                store
                    .lock()
                    .expect("store lock must be available")
                    .snapshot()
            })
    });
    let error = || {
        execution.budget.0.clone().fail();
        ScriptError::new("contract", "subscription setup field required")
    };
    if !Arc::ptr_eq(scripts.contract().ir(), snapshot.ir()) {
        return Err(error());
    }
    let Some(FieldPlan::Script(id)) = scripts.contract().ir().field_plan(field) else {
        return Err(error());
    };
    let script = scripts.scripts().get(id.0).ok_or_else(error)?;
    let (limits, retention) = store
        .lock()
        .map_err(|_| source_failure(PublicationError::Invariant))?
        .source_policy();
    runtime::execute_subscription_setup(script, arguments, &snapshot, execution, limits, retention)
}

pub(super) fn establish(
    store: &Arc<Mutex<PublicationStore>>,
    scripts: &ScriptSet,
    input: EstablishInput<'_>,
) -> Result<EstablishedSubscription, ScriptError> {
    let EstablishInput {
        request,
        field,
        arguments,
        snapshot,
        execution,
        observation,
    } = input;
    let binding = setup(
        store, scripts, request, field, arguments, snapshot, execution,
    )?;
    let Some(FieldPlan::Script(expected)) = scripts.contract().ir().field_plan(field) else {
        return Err(contract_failure(execution));
    };
    if binding.script() != *expected {
        return Err(contract_failure(execution));
    }
    let owner = store
        .lock()
        .map_err(|_| source_failure(PublicationError::Invariant))?
        .registration_owner();
    let prepared = match (binding.topic(), binding.sequence()) {
        (Some(topic), None) => owner.prepare(scripts.contract().ir(), field, topic),
        (None, Some(definition)) => {
            owner.prepare_sequence(scripts.contract().ir(), field, definition)
        }
        _ => return Err(contract_failure(execution)),
    }
    .map_err(source_failure)?;
    let overflow = binding.overflow();
    let registration = {
        let mut store = store
            .lock()
            .map_err(|_| source_failure(PublicationError::Invariant))?;
        store.register_source(prepared, overflow).map(|registered| {
            // The store issues the generation first, then this fixed host fence
            // selects it before either lock is released. No VM or log work runs
            // here; source finalization and notification remain below.
            let signal = observation
                .as_ref()
                .map(|fence| {
                    fence.registered(
                        registered.generation(),
                        overflow,
                        registered.source_delivery(),
                    )
                })
                .transpose();
            (registered, signal)
        })
    };
    let (registered, signal) = registration.map_err(|failure| source_failure(failure.finish()))?;
    let signal = match signal {
        Ok(signal) => signal,
        Err(()) => {
            drop(registered.finish());
            return Err(source_failure(PublicationError::Invariant));
        }
    };
    let source = registered.finish();
    if let Some(signal) = &signal {
        signal.notify();
    }
    Ok(EstablishedSubscription::new(source, signal))
}

fn contract_failure(execution: &Execution) -> ScriptError {
    execution.budget.0.clone().fail();
    ScriptError::new("contract", "subscription binding differs from setup field")
}

fn source_failure(_failure: PublicationError) -> ScriptError {
    ScriptError::new("source", "subscription source registration failed")
}
