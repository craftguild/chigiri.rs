//! Establish each named source without activating any partially prepared clock.

use std::sync::{Arc, Mutex};

use super::{ConsumerHost, Entry, execution, log};
use crate::cancellation::Cancellation;
use crate::luau::{ConsumerScript, ScriptError};
use crate::publication::PublicationStore;
use crate::runtime;

pub(super) fn prepare(
    store: &Arc<Mutex<PublicationStore>>,
    consumer: ConsumerScript,
    host: &ConsumerHost,
    shutdown: &Cancellation,
) -> Result<Entry, ScriptError> {
    let script = consumer.script();
    let execution = execution(host, &consumer, shutdown);
    let (snapshot, policy, owner) = {
        let store = store.lock().map_err(|_| failure())?;
        (
            store.snapshot(),
            store.source_policy(),
            store.registration_owner(),
        )
    };
    let binding = runtime::execute_consumer_setup(
        &consumer,
        &snapshot,
        &execution,
        policy.0,
        policy.1,
        &log(host, &consumer, crate::api::consumers::SUBSCRIBE),
    )?;
    if binding.script() != script.id() {
        return Err(script.error("contract", "consumer source destination mismatch", 1, 1));
    }
    let field = consumer.registration().field();
    let prepared = match (binding.topic(), binding.sequence(), binding.all_topics()) {
        (Some(topic), None, false) => owner.prepare(script.ir(), field, topic),
        (None, Some(definition), false) => owner.prepare_sequence(script.ir(), field, definition),
        (None, None, true) => owner.prepare_all_topics(script.ir(), field),
        _ => return Err(failure()),
    }
    .map_err(|_| failure())?;
    let result = {
        store
            .lock()
            .map_err(|_| failure())?
            .register_source(prepared, binding.overflow())
    };
    let registered = result.map_err(|failure| {
        let _ = failure.finish();
        script.error("source", "consumer source admission failed", 1, 1)
    })?;
    Ok(Entry {
        log: log(host, &consumer, crate::api::consumers::CONSUME),
        script: consumer,
        source: Some(registered.finish_deferred()),
        invocation: 0,
    })
}

fn failure() -> ScriptError {
    ScriptError::new("source", "consumer source startup failed")
}
