//! Confine the VM and mutable store to one dedicated thread behind a bounded, cancellation-aware job queue.

#[cfg(test)]
mod admission_tests;
mod client;
mod consumers;
mod delivery;
#[cfg(test)]
mod delivery_tests;
mod event_lifecycle;
#[cfg(test)]
mod hook_deadline_tests;
#[cfg(test)]
mod hook_tests;
mod jobs;
mod lifecycle;
#[cfg(test)]
mod measurements;
mod pending;
mod rejections;
mod request;
mod startup;
#[cfg(test)]
#[path = "worker/subscription_tests/mod.rs"]
mod subscription_tests;
pub use client::Resolution;

use jobs::{Job, run};
use std::sync::Arc;
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender, bounded};

use crate::cancellation::Cancellation;
use crate::ir::MockIr;
use crate::luau::{HookScript, ScriptError, ScriptSet};
use crate::publication::PublicationStore;
use crate::store::Store;

/// Client handle that owns neither Luau values nor the mutable store.
#[derive(Clone)]
pub struct Client {
    ir: Arc<MockIr>,
    sender: Sender<Job>,
    queue_open: Arc<std::sync::Mutex<bool>>,
    initial: crate::hooks::StateContract,
    pub(in crate::service) clock: Arc<crate::clock::ServerClock>,
    control_serial: Arc<std::sync::Mutex<()>>,
    hooks: std::collections::BTreeSet<crate::hooks::HookSlot>,
    mutation: Arc<tokio::sync::Mutex<()>>,
    events: Arc<event_lifecycle::EventLifecycle>,
    mutation_waiters: Arc<tokio::sync::Semaphore>,
    committed: Arc<std::sync::Mutex<PublicationStore>>,
    request: Option<Arc<request::Request>>,
    diagnostics: Option<Arc<dyn Fn(ScriptError) + Send + Sync>>,
    pub(in crate::service) session: Option<Arc<super::hooks::Session>>,
    /// Sticky shutdown signal that prevents new commits.
    pub shutdown: Cancellation,
}

/// One lifecycle owner for the VM and its independent bounded delivery driver.
pub struct Worker {
    /// Owned client to endpoint.
    pub client: Client,
    thread: Option<JoinHandle<()>>,
    delivery: Option<JoinHandle<()>>,
}

impl Worker {
    /// Obtain the host-side controller for this server instance.
    pub fn control(&self) -> super::ServerControl {
        super::ServerControl::shared(
            self.client.clock.clone(),
            self.client.control_serial.clone(),
            self.client.shutdown.clone(),
        )
    }

    fn launch(
        ir: Arc<MockIr>,
        initial: crate::hooks::StateContract,
        hooks: Vec<HookScript>,
        store: Store,
        settings: startup::LaunchSettings,
        build: impl FnOnce() -> Result<ScriptSet, ScriptError> + Send + 'static,
    ) -> Result<Self, ScriptError> {
        let mut slots = std::collections::BTreeSet::new();
        for hook in &hooks {
            if !slots.insert(hook.slot())
                || !hook.accepts(&store.snapshot(), &initial.instantiate())
            {
                return Err(environment("worker hook contract differs"));
            }
        }
        let (mut client, receiver) = queue_with_store(ir, store, settings.source_limits)?;
        client.clock = crate::clock::ServerClock::new(&settings.clock);
        let consumer_host = consumers::ConsumerHost {
            clock: client.clock.clone(),
            mutation: client.mutation.clone(),
            events: client.events.clone(),
            logger: settings.logger,
        };
        client.initial = initial;
        client.hooks = hooks.iter().map(HookScript::slot).collect();
        let (ready, started) = std::sync::mpsc::sync_channel(1);
        let stopping = client.shutdown.clone();
        let queue_open = client.queue_open.clone();
        let committed = client.committed.clone();
        let thread = std::thread::Builder::new()
            .name("chigiri-mock-vm".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let scripts = match build() {
                    Ok(scripts) => scripts,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                let consumers = match consumers::ConsumerSet::prepare(
                    &committed,
                    settings.consumers,
                    consumer_host,
                    &stopping,
                ) {
                    Ok(consumers) => consumers,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                if ready.send(Ok(())).is_ok() {
                    run(
                        committed, scripts, hooks, receiver, stopping, queue_open, consumers,
                    );
                }
            })
            .map_err(|_| environment("cannot create VM worker"))?;
        let mut worker = Self {
            client,
            thread: Some(thread),
            delivery: None,
        };
        match started.recv() {
            Ok(Ok(())) => {
                worker.delivery = Some(delivery::start(&worker.client)?);
                Ok(worker)
            }
            Ok(Err(error)) => {
                worker.join()?;
                Err(error)
            }
            Err(_) => {
                worker.join()?;
                Err(environment("VM startup failed"))
            }
        }
    }
}

fn environment(message: &'static str) -> ScriptError {
    ScriptError::new("environment", message)
}

fn reply_missing() -> ScriptError {
    environment("worker reply missing")
}

fn queue_with_store(
    ir: Arc<MockIr>,
    store: Store,
    source_limits: crate::events::SourceLimits,
) -> Result<(Client, Receiver<Job>), ScriptError> {
    let committed = PublicationStore::with_source_limits(
        store,
        chigiri::SubscriptionLimits::default().max_active_subscriptions,
        source_limits,
    )
    .map_err(|_| environment("cannot initialize publication ownership"))?;
    let (sender, receiver) = bounded(crate::limits::QUEUED_JOBS);
    Ok((
        Client {
            committed: Arc::new(std::sync::Mutex::new(committed)),
            request: None,
            diagnostics: None,
            session: None,
            ir,
            sender,
            queue_open: Arc::new(std::sync::Mutex::new(true)),
            initial: Default::default(),
            clock: crate::clock::ServerClock::new(&crate::clock::ClockSettings::system()),
            control_serial: Arc::new(std::sync::Mutex::new(())),
            hooks: Default::default(),
            mutation: Arc::default(),
            events: Arc::default(),
            mutation_waiters: Arc::new(tokio::sync::Semaphore::new(crate::limits::QUEUED_JOBS)),
            shutdown: Cancellation::default(),
        },
        receiver,
    ))
}

#[cfg(test)]
fn queue(ir: Arc<MockIr>) -> (Client, Receiver<Job>) {
    queue_with_store(
        ir.clone(),
        Store::empty(ir),
        crate::events::SourceLimits::default(),
    )
    .unwrap()
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
