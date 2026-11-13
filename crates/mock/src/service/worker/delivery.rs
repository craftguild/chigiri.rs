//! Progress bounded captured delivery independently of synchronous Luau jobs.

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use super::{Client, environment};
use crate::cancellation::{CancelOnDrop, Cancellation};
use crate::luau::ScriptError;
use crate::publication::{PublicationPump, PublicationStore};

const IDLE_WAIT: Duration = Duration::from_millis(20);

pub(super) fn start(client: &Client) -> Result<JoinHandle<()>, ScriptError> {
    let store = client.committed.clone();
    let shutdown = client.shutdown.clone();
    let events = client.events.clone();
    std::thread::Builder::new()
        .name("chigiri-mock-delivery".into())
        .spawn(move || run(store, shutdown, events))
        .map_err(|_| environment("cannot create source delivery worker"))
}

fn run(
    store: Arc<Mutex<PublicationStore>>,
    shutdown: Cancellation,
    events: Arc<super::event_lifecycle::EventLifecycle>,
) {
    // The fixed driver is joined by Worker. It retains no VM, callback,
    // request, or candidate, and a failure cancels its VM peer as well.
    let _cancel = CancelOnDrop(shutdown.clone());
    let wake = match store.lock() {
        Ok(store) => store.delivery_wake(),
        Err(_) => return,
    };
    if wake.install().is_err() {
        return;
    }
    let mut pump = PublicationPump::new();
    while !shutdown.cancelled() {
        if events.stopped() {
            // Graceful finishes only the already-running callback. Waiting
            // queues are retired by that scheduler; no new ticks or chains run.
            std::thread::park_timeout(IDLE_WAIT);
            continue;
        }
        match pump.step(&store) {
            Ok(true) => {}
            Ok(false) => match pump.wait_duration(&store, std::time::Instant::now(), IDLE_WAIT) {
                Ok(wait) => std::thread::park_timeout(wait),
                Err(_) => break,
            },
            Err(_) => break,
        }
    }
    shutdown.cancel();
    while let Ok(true) = pump.shutdown_step(&store) {}
}
