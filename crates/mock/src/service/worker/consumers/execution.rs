//! Commit one fresh consumer candidate and its publications under the existing fence.

use std::sync::{Arc, Mutex};

use super::{ConsumerHost, Entry, execution};
use crate::cancellation::Cancellation;
use crate::luau::ScriptError;
use crate::publication::{CapturedEvent, PublicationStore};
use crate::runtime;
use crate::value::ValueBudget;

pub(super) fn run(
    store: &Arc<Mutex<PublicationStore>>,
    entry: &Entry,
    event: &CapturedEvent,
    host: &ConsumerHost,
    shutdown: &Cancellation,
) -> Result<(), ScriptError> {
    let script = entry.script.script();
    let failure = |phase, message| script.error(phase, message, 1, 1);
    let mut execution = execution(host, &entry.script, shutdown);
    if let Some(source) = event.cancellation() {
        execution.cancel = execution.cancel.linked(source);
    }
    let (mut candidate, owner, staging) = {
        let store = store
            .lock()
            .map_err(|_| failure("host", "consumer store unavailable"))?;
        if !store.owns_event(event) {
            return Err(failure(
                "contract",
                "consumer event belongs to another worker",
            ));
        }
        store
            .mutation_admission()
            .map_err(|_| failure("source", "consumer publication ordering unavailable"))?;
        (
            store.begin_request(ValueBudget::with_work(execution.budget.0.clone())),
            store.capture_owner(),
            store.staging(),
        )
    };
    let publications = runtime::execute_consumer(
        &entry.script,
        event,
        candidate.store_mut(),
        &execution,
        &owner,
        &entry.log,
    )?;
    let prepared = candidate
        .prepare()
        .map_err(|_| failure("invariant", "consumer candidate validation failed"))?;
    let mut publications = publications
        .prepare(&staging)
        .map_err(|_| failure("source", "consumer publication reservation failed"))?;
    let result = execution
        .cancel
        .commit(&execution.shutdown, execution.deadline, || {
            store
                .lock()
                .map_err(|_| failure("host", "consumer store unavailable"))?
                .commit_publications(&prepared, &mut publications)
                .map_err(|_| failure("invariant", "consumer commit rejected"))
        })
        .ok_or_else(|| failure("cancel", "consumer cancelled before commit"))??;
    // Retired store/capture owners leave the lock and cancellation fence before
    // disposal. Logging never changes an already won commit or waits on I/O.
    drop(result);
    execution.budget.0.observe_elapsed_soft();
    execution.budget.seal_clock();
    Ok(())
}
